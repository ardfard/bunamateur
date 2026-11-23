use crate::db::{
    AppState, build_review_queue, compute_dashboard, detect_grammar_in_text, get_grammar_detail,
    list_conjugations, list_grammar_summaries, list_reading_passages, load_known_kanji,
    load_wanikani_status, save_wanikani_sync,
};
use crate::models::{
    ReadingAnalyzeRequest, ReadingAnalyzeResponse, StartLessonRequest, SubmitAnswerRequest,
    SubmitAnswerResponse, UndoReviewRequest, UpdateNotesRequest, WaniKaniConnectRequest,
    WaniKaniSettingsRequest,
};
use crate::srs::{check_answer, compute_srs_transition, srs_interval_hours, srs_stage_label};
use crate::wanikani::{compute_kanji_readiness, generate_demo_wanikani_sync, sync_from_wanikani};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post, put},
};
use chrono::{Duration, Utc};
use rusqlite::params;
use serde::Deserialize;
use serde_json::json;

pub fn api_router() -> Router<AppState> {
    Router::new()
        .route("/dashboard", get(get_dashboard))
        .route("/reviews", get(get_reviews))
        .route("/reviews/submit", post(submit_review))
        .route("/reviews/undo", post(undo_review))
        .route("/lessons/start", post(start_lesson))
        .route("/grammar", get(get_grammar_list))
        .route("/grammar/{id}", get(get_grammar_item))
        .route("/grammar/{id}/notes", put(update_grammar_notes))
        .route("/grammar/{id}/reset", post(reset_grammar_progress))
        .route("/reading/passages", get(get_reading_passages))
        .route("/reading/analyze", post(analyze_reading))
        .route("/conjugation", get(get_conjugation_items))
        .route("/wanikani/status", get(get_wanikani_status))
        .route("/wanikani/connect", post(connect_wanikani))
        .route("/wanikani/sync", post(resync_wanikani))
        .route("/wanikani/demo", post(demo_wanikani))
        .route("/wanikani/disconnect", post(disconnect_wanikani))
        .route("/wanikani/settings", put(update_wanikani_settings))
}

fn err_response(status: StatusCode, msg: impl Into<String>) -> (StatusCode, Json<serde_json::Value>) {
    (status, Json(json!({ "error": msg.into() })))
}

async fn get_dashboard(State(state): State<AppState>) -> impl IntoResponse {
    let conn = state.db.lock().unwrap();
    match compute_dashboard(&conn) {
        Ok(stats) => (StatusCode::OK, Json(json!(stats))),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[derive(Deserialize)]
struct ReviewQuery {
    mode: Option<String>,
    jlpt: Option<String>,
    limit: Option<usize>,
}

async fn get_reviews(
    State(state): State<AppState>,
    Query(q): Query<ReviewQuery>,
) -> impl IntoResponse {
    let conn = state.db.lock().unwrap();
    let mode = q.mode.as_deref().unwrap_or("review");
    let limit = q.limit.unwrap_or(20).clamp(1, 50);
    match build_review_queue(&conn, mode, q.jlpt.as_deref(), limit) {
        Ok(items) => (StatusCode::OK, Json(json!(items))),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn submit_review(
    State(state): State<AppState>,
    Json(req): Json<SubmitAnswerRequest>,
) -> impl IntoResponse {
    let conn = state.db.lock().unwrap();
    let sent_res = conn.query_row(
        "SELECT answer, alt_answers FROM grammar_sentences WHERE id = ?1",
        [req.sentence_id],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
    );

    let (primary, alt_raw) = match sent_res {
        Ok(v) => v,
        Err(_) => return err_response(StatusCode::NOT_FOUND, "Sentence not found"),
    };
    let alts: Vec<String> = serde_json::from_str(&alt_raw).unwrap_or_default();

    let prog_res = conn.query_row(
        "SELECT srs_stage, ghost_level FROM user_progress WHERE grammar_id = ?1",
        [req.grammar_id],
        |r| Ok((r.get::<_, i32>(0)?, r.get::<_, i32>(1)?)),
    );
    let (prev_srs, prev_ghost) = match prog_res {
        Ok(v) => v,
        Err(_) => return err_response(StatusCode::NOT_FOUND, "Grammar progress not found"),
    };

    let matched = check_answer(&req.user_input, &primary, &alts);
    let correct = matched.is_some();
    let now = Utc::now();

    if req.mode == "cram" {
        let feedback = if correct {
            "Correct! (Cram mode does not alter SRS schedules)".to_string()
        } else {
            format!("Expected 「{}」. Keep drilling in Cram mode!", primary)
        };
        let resp = SubmitAnswerResponse {
            correct,
            matched_answer: matched.unwrap_or_default(),
            expected_answer: primary,
            alt_answers: alts,
            previous_srs_stage: prev_srs,
            new_srs_stage: prev_srs,
            new_srs_label: srs_stage_label(prev_srs),
            next_review_at: None,
            previous_ghost_level: prev_ghost,
            new_ghost_level: prev_ghost,
            feedback,
            review_log_id: None,
        };
        return (StatusCode::OK, Json(json!(resp)));
    }

    let transition = compute_srs_transition(prev_srs, prev_ghost, req.is_ghost, correct, now);
    let next_review_str = transition.next_review_at.map(|d| d.to_rfc3339());
    let ghost_due_str = transition.ghost_due_at.map(|d| d.to_rfc3339());

    let update_res = if req.is_ghost {
        conn.execute(
            "UPDATE user_progress
             SET ghost_level = ?1,
                 ghost_due_at = ?2,
                 correct_count = correct_count + ?3,
                 incorrect_count = incorrect_count + ?4
             WHERE grammar_id = ?5",
            params![
                transition.new_ghost_level,
                ghost_due_str,
                if correct { 1 } else { 0 },
                if correct { 0 } else { 1 },
                req.grammar_id
            ],
        )
    } else {
        conn.execute(
            "UPDATE user_progress
             SET srs_stage = ?1,
                 next_review_at = ?2,
                 ghost_level = ?3,
                 ghost_due_at = COALESCE(?4, ghost_due_at),
                 correct_count = correct_count + ?5,
                 incorrect_count = incorrect_count + ?6,
                 streak = CASE WHEN ?5 = 1 THEN streak + 1 ELSE 0 END
             WHERE grammar_id = ?7",
            params![
                transition.new_stage,
                next_review_str,
                transition.new_ghost_level,
                ghost_due_str,
                if correct { 1 } else { 0 },
                if correct { 0 } else { 1 },
                req.grammar_id
            ],
        )
    };

    if let Err(e) = update_res {
        return err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
    }

    let _ = conn.execute(
        "INSERT INTO review_logs (grammar_id, correct, mode, is_ghost, reviewed_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            req.grammar_id,
            if correct { 1 } else { 0 },
            req.mode,
            if req.is_ghost { 1 } else { 0 },
            now.to_rfc3339()
        ],
    );
    let log_id = Some(conn.last_insert_rowid());

    let feedback = if req.is_ghost {
        if correct {
            if transition.new_ghost_level == 0 {
                "Ghost exorcised! You mastered this weak point.".to_string()
            } else {
                format!("Ghost weakened -> Level {}!", transition.new_ghost_level)
            }
        } else {
            format!("Expected 「{}」. Ghost remains active for extra practice.", primary)
        }
    } else if correct {
        format!("Promoted to {}!", srs_stage_label(transition.new_stage))
    } else {
        format!(
            "Expected 「{}」. Spawned a Ghost Review so you'll lock it in.",
            primary
        )
    };

    let resp = SubmitAnswerResponse {
        correct,
        matched_answer: matched.unwrap_or_default(),
        expected_answer: primary,
        alt_answers: alts,
        previous_srs_stage: prev_srs,
        new_srs_stage: transition.new_stage,
        new_srs_label: srs_stage_label(transition.new_stage),
        next_review_at: next_review_str,
        previous_ghost_level: prev_ghost,
        new_ghost_level: transition.new_ghost_level,
        feedback,
        review_log_id: log_id,
    };

    (StatusCode::OK, Json(json!(resp)))
}

async fn undo_review(
    State(state): State<AppState>,
    Json(req): Json<UndoReviewRequest>,
) -> impl IntoResponse {
    let conn = state.db.lock().unwrap();
    let now = Utc::now();
    let restore_due = if req.previous_srs_stage > 0 {
        Some((now - Duration::minutes(1)).to_rfc3339())
    } else {
        None
    };
    let restore_ghost_due = if req.previous_ghost_level > 0 {
        Some((now - Duration::minutes(1)).to_rfc3339())
    } else {
        None
    };

    let _ = conn.execute(
        "UPDATE user_progress SET srs_stage = ?1, next_review_at = ?2, ghost_level = ?3, ghost_due_at = ?4 WHERE grammar_id = ?5",
        params![
            req.previous_srs_stage,
            restore_due,
            req.previous_ghost_level,
            restore_ghost_due,
            req.grammar_id
        ],
    );
    if let Some(lid) = req.review_log_id {
        let _ = conn.execute("DELETE FROM review_logs WHERE id = ?1", [lid]);
    }

    (StatusCode::OK, Json(json!({ "ok": true })))
}

async fn start_lesson(
    State(state): State<AppState>,
    Json(req): Json<StartLessonRequest>,
) -> impl IntoResponse {
    let conn = state.db.lock().unwrap();
    let next_due = (Utc::now() + Duration::hours(srs_interval_hours(1).unwrap_or(4))).to_rfc3339();
    match conn.execute(
        "UPDATE user_progress SET srs_stage = CASE WHEN srs_stage = 0 THEN 1 ELSE srs_stage END, next_review_at = COALESCE(next_review_at, ?1) WHERE grammar_id = ?2",
        params![next_due, req.grammar_id],
    ) {
        Ok(_) => (StatusCode::OK, Json(json!({ "ok": true, "next_review_at": next_due }))),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[derive(Deserialize)]
struct GrammarQuery {
    jlpt: Option<String>,
    q: Option<String>,
    status: Option<String>,
}

async fn get_grammar_list(
    State(state): State<AppState>,
    Query(q): Query<GrammarQuery>,
) -> impl IntoResponse {
    let conn = state.db.lock().unwrap();
    match list_grammar_summaries(&conn, q.jlpt.as_deref(), q.q.as_deref(), q.status.as_deref()) {
        Ok(items) => (StatusCode::OK, Json(json!(items))),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn get_grammar_item(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    let conn = state.db.lock().unwrap();
    match get_grammar_detail(&conn, id) {
        Ok(Some(detail)) => (StatusCode::OK, Json(json!(detail))),
        Ok(None) => err_response(StatusCode::NOT_FOUND, "Grammar point not found"),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn update_grammar_notes(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<UpdateNotesRequest>,
) -> impl IntoResponse {
    let conn = state.db.lock().unwrap();
    match conn.execute(
        "UPDATE user_progress SET user_notes = ?1 WHERE grammar_id = ?2",
        params![req.notes, id],
    ) {
        Ok(_) => (StatusCode::OK, Json(json!({ "ok": true }))),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn reset_grammar_progress(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    let conn = state.db.lock().unwrap();
    match conn.execute(
        "UPDATE user_progress SET srs_stage = 0, next_review_at = NULL, ghost_level = 0, ghost_due_at = NULL, streak = 0 WHERE grammar_id = ?1",
        [id],
    ) {
        Ok(_) => (StatusCode::OK, Json(json!({ "ok": true }))),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn get_reading_passages(State(state): State<AppState>) -> impl IntoResponse {
    let conn = state.db.lock().unwrap();
    match list_reading_passages(&conn) {
        Ok(items) => (StatusCode::OK, Json(json!(items))),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn analyze_reading(
    State(state): State<AppState>,
    Json(req): Json<ReadingAnalyzeRequest>,
) -> impl IntoResponse {
    let conn = state.db.lock().unwrap();
    let known = match load_known_kanji(&conn) {
        Ok(k) => k,
        Err(e) => return err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    let (known_cnt, total_cnt, pct, unknown) = compute_kanji_readiness(&req.text, &known);
    let detected = match detect_grammar_in_text(&conn, &req.text) {
        Ok(d) => d,
        Err(e) => return err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };

    let resp = ReadingAnalyzeResponse {
        total_kanji: total_cnt,
        known_kanji: known_cnt,
        unknown_kanji: unknown,
        wk_readiness_pct: pct,
        detected_grammar: detected,
    };
    (StatusCode::OK, Json(json!(resp)))
}

#[derive(Deserialize)]
struct ConjugationQuery {
    form: Option<String>,
}

async fn get_conjugation_items(
    State(state): State<AppState>,
    Query(q): Query<ConjugationQuery>,
) -> impl IntoResponse {
    let conn = state.db.lock().unwrap();
    match list_conjugations(&conn, q.form.as_deref()) {
        Ok(items) => (StatusCode::OK, Json(json!(items))),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn get_wanikani_status(State(state): State<AppState>) -> impl IntoResponse {
    let conn = state.db.lock().unwrap();
    match load_wanikani_status(&conn) {
        Ok(status) => (StatusCode::OK, Json(json!(status))),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn connect_wanikani(
    State(state): State<AppState>,
    Json(req): Json<WaniKaniConnectRequest>,
) -> impl IntoResponse {
    let token = req.api_token.trim().to_string();
    if token.is_empty() {
        return err_response(StatusCode::BAD_REQUEST, "API token is required");
    }
    match sync_from_wanikani(&token).await {
        Ok(sync_result) => {
            let mut conn = state.db.lock().unwrap();
            match save_wanikani_sync(&mut conn, Some(&token), false, &sync_result) {
                Ok(status) => (StatusCode::OK, Json(json!(status))),
                Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
            }
        }
        Err(e) => err_response(StatusCode::BAD_GATEWAY, e.to_string()),
    }
}

async fn resync_wanikani(State(state): State<AppState>) -> impl IntoResponse {
    let (token, is_demo): (Option<String>, i32) = {
        let conn = state.db.lock().unwrap();
        match conn.query_row(
            "SELECT api_token, is_demo FROM wk_profile WHERE id = 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        ) {
            Ok(v) => v,
            Err(e) => return err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        }
    };

    if is_demo == 1 {
        let demo = generate_demo_wanikani_sync();
        let mut conn = state.db.lock().unwrap();
        return match save_wanikani_sync(&mut conn, None, true, &demo) {
            Ok(status) => (StatusCode::OK, Json(json!(status))),
            Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        };
    }

    let Some(tok) = token else {
        return err_response(StatusCode::BAD_REQUEST, "WaniKani is not connected yet");
    };

    match sync_from_wanikani(&tok).await {
        Ok(sync_result) => {
            let mut conn = state.db.lock().unwrap();
            match save_wanikani_sync(&mut conn, Some(&tok), false, &sync_result) {
                Ok(status) => (StatusCode::OK, Json(json!(status))),
                Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
            }
        }
        Err(e) => err_response(StatusCode::BAD_GATEWAY, e.to_string()),
    }
}

async fn demo_wanikani(State(state): State<AppState>) -> impl IntoResponse {
    let demo = generate_demo_wanikani_sync();
    let mut conn = state.db.lock().unwrap();
    match save_wanikani_sync(&mut conn, None, true, &demo) {
        Ok(status) => (StatusCode::OK, Json(json!(status))),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn disconnect_wanikani(State(state): State<AppState>) -> impl IntoResponse {
    let conn = state.db.lock().unwrap();
    let _ = conn.execute(
        "UPDATE wk_profile SET api_token = NULL, is_demo = 0, username = NULL, level = 0, lessons_available = 0, reviews_available = 0, last_synced_at = NULL WHERE id = 1",
        [],
    );
    let _ = conn.execute("DELETE FROM wk_subjects", []);
    match load_wanikani_status(&conn) {
        Ok(status) => (StatusCode::OK, Json(json!(status))),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn update_wanikani_settings(
    State(state): State<AppState>,
    Json(req): Json<WaniKaniSettingsRequest>,
) -> impl IntoResponse {
    let conn = state.db.lock().unwrap();
    let _ = conn.execute(
        "UPDATE wk_profile SET smart_furigana = ?1, highlight_known_kanji = ?2 WHERE id = 1",
        params![
            if req.smart_furigana { 1 } else { 0 },
            if req.highlight_known_kanji { 1 } else { 0 }
        ],
    );
    match load_wanikani_status(&conn) {
        Ok(status) => (StatusCode::OK, Json(json!(status))),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

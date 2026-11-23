use crate::db::{
    AppState, build_review_queue, compute_dashboard, detect_grammar_in_text,
    disconnect_wanikani as db_disconnect_wanikani, get_grammar_detail, get_sentence_answers,
    get_user_progress_stages, get_wanikani_credentials, list_conjugations, list_grammar_summaries,
    list_reading_passages, load_known_kanji, load_wanikani_status, record_review_submission,
    reset_grammar_progress as db_reset_grammar_progress, save_wanikani_sync,
    start_lesson as db_start_lesson, undo_review as db_undo_review,
    update_grammar_notes as db_update_grammar_notes,
    update_wanikani_settings as db_update_wanikani_settings,
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
    match compute_dashboard(&state.pool).await {
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
    let mode = q.mode.as_deref().unwrap_or("review");
    let limit = q.limit.unwrap_or(20).clamp(1, 50);
    match build_review_queue(&state.pool, mode, q.jlpt.as_deref(), limit).await {
        Ok(items) => (StatusCode::OK, Json(json!(items))),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn submit_review(
    State(state): State<AppState>,
    Json(req): Json<SubmitAnswerRequest>,
) -> impl IntoResponse {
    let (primary, alts) = match get_sentence_answers(&state.pool, req.sentence_id).await {
        Ok(Some(v)) => v,
        Ok(None) => return err_response(StatusCode::NOT_FOUND, "Sentence not found"),
        Err(e) => return err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };

    let (prev_srs, prev_ghost) = match get_user_progress_stages(&state.pool, req.grammar_id).await {
        Ok(Some(v)) => v,
        Ok(None) => return err_response(StatusCode::NOT_FOUND, "Grammar progress not found"),
        Err(e) => return err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
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

    let log_id = match record_review_submission(
        &state.pool,
        req.grammar_id,
        &req.mode,
        req.is_ghost,
        correct,
        &transition,
        now,
    )
    .await
    {
        Ok(id) => Some(id),
        Err(e) => return err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };

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
    let now = Utc::now();
    match db_undo_review(
        &state.pool,
        req.grammar_id,
        req.previous_srs_stage,
        req.previous_ghost_level,
        req.review_log_id,
        now,
    )
    .await
    {
        Ok(()) => (StatusCode::OK, Json(json!({ "ok": true }))),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn start_lesson(
    State(state): State<AppState>,
    Json(req): Json<StartLessonRequest>,
) -> impl IntoResponse {
    let next_due = Utc::now() + Duration::hours(srs_interval_hours(1).unwrap_or(4));
    match db_start_lesson(&state.pool, req.grammar_id, next_due).await {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({ "ok": true, "next_review_at": next_due.to_rfc3339() })),
        ),
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
    match list_grammar_summaries(
        &state.pool,
        q.jlpt.as_deref(),
        q.q.as_deref(),
        q.status.as_deref(),
    )
    .await
    {
        Ok(items) => (StatusCode::OK, Json(json!(items))),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn get_grammar_item(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    match get_grammar_detail(&state.pool, id).await {
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
    match db_update_grammar_notes(&state.pool, id, &req.notes).await {
        Ok(()) => (StatusCode::OK, Json(json!({ "ok": true }))),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn reset_grammar_progress(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    match db_reset_grammar_progress(&state.pool, id).await {
        Ok(()) => (StatusCode::OK, Json(json!({ "ok": true }))),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn get_reading_passages(State(state): State<AppState>) -> impl IntoResponse {
    match list_reading_passages(&state.pool).await {
        Ok(items) => (StatusCode::OK, Json(json!(items))),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn analyze_reading(
    State(state): State<AppState>,
    Json(req): Json<ReadingAnalyzeRequest>,
) -> impl IntoResponse {
    let known = match load_known_kanji(&state.pool).await {
        Ok(k) => k,
        Err(e) => return err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    let (known_cnt, total_cnt, pct, unknown) = compute_kanji_readiness(&req.text, &known);
    let detected = match detect_grammar_in_text(&state.pool, &req.text).await {
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
    match list_conjugations(&state.pool, q.form.as_deref()).await {
        Ok(items) => (StatusCode::OK, Json(json!(items))),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn get_wanikani_status(State(state): State<AppState>) -> impl IntoResponse {
    match load_wanikani_status(&state.pool).await {
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
        Ok(sync_result) => match save_wanikani_sync(&state.pool, Some(&token), false, &sync_result).await {
            Ok(status) => (StatusCode::OK, Json(json!(status))),
            Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        },
        Err(e) => err_response(StatusCode::BAD_GATEWAY, e.to_string()),
    }
}

async fn resync_wanikani(State(state): State<AppState>) -> impl IntoResponse {
    let (token, is_demo) = match get_wanikani_credentials(&state.pool).await {
        Ok(v) => v,
        Err(e) => return err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };

    if is_demo {
        let demo = generate_demo_wanikani_sync();
        return match save_wanikani_sync(&state.pool, None, true, &demo).await {
            Ok(status) => (StatusCode::OK, Json(json!(status))),
            Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        };
    }

    let Some(tok) = token else {
        return err_response(StatusCode::BAD_REQUEST, "WaniKani is not connected yet");
    };

    match sync_from_wanikani(&tok).await {
        Ok(sync_result) => match save_wanikani_sync(&state.pool, Some(&tok), false, &sync_result).await {
            Ok(status) => (StatusCode::OK, Json(json!(status))),
            Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        },
        Err(e) => err_response(StatusCode::BAD_GATEWAY, e.to_string()),
    }
}

async fn demo_wanikani(State(state): State<AppState>) -> impl IntoResponse {
    let demo = generate_demo_wanikani_sync();
    match save_wanikani_sync(&state.pool, None, true, &demo).await {
        Ok(status) => (StatusCode::OK, Json(json!(status))),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn disconnect_wanikani(State(state): State<AppState>) -> impl IntoResponse {
    match db_disconnect_wanikani(&state.pool).await {
        Ok(status) => (StatusCode::OK, Json(json!(status))),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn update_wanikani_settings(
    State(state): State<AppState>,
    Json(req): Json<WaniKaniSettingsRequest>,
) -> impl IntoResponse {
    match db_update_wanikani_settings(&state.pool, req.smart_furigana, req.highlight_known_kanji)
        .await
    {
        Ok(status) => (StatusCode::OK, Json(json!(status))),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

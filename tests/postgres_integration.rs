use bunamateur::{
    db::{
        AppState, build_review_queue, compute_dashboard, detect_grammar_in_text,
        get_grammar_detail, list_conjugations, list_grammar_summaries, list_reading_passages,
        load_wanikani_status, run_migrations, sanitize_database_url, seed_content,
        update_grammar_notes,
    },
    routes,
    seed_data::{SEED_CONJUGATIONS, SEED_GRAMMAR, SEED_PASSAGES},
};
use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgPoolOptions};
use tokio::net::TcpListener;

const DEFAULT_TEST_DATABASE_URL: &str = "postgres://ardfard@127.0.0.1:5432/bunamateur_test";

fn test_database_url() -> String {
    std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| DEFAULT_TEST_DATABASE_URL.to_string())
}

async fn reset_and_connect_test_db() -> PgPool {
    let url = test_database_url();
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await
        .expect("Failed to connect to bunamateur_test PostgreSQL database");

    // Reset public schema completely so embedded migrations are exercised from scratch
    sqlx::query("DROP SCHEMA IF EXISTS public CASCADE")
        .execute(&pool)
        .await
        .expect("Failed to drop test schema");
    sqlx::query("CREATE SCHEMA public")
        .execute(&pool)
        .await
        .expect("Failed to recreate test schema");

    pool
}

#[test]
fn test_sanitize_database_url_redacts_password() {
    assert_eq!(
        sanitize_database_url("postgres://ardfard:s3cr3t@127.0.0.1:5432/bunamateur_dev"),
        "postgres://ardfard@127.0.0.1:5432/bunamateur_dev"
    );
    assert_eq!(
        sanitize_database_url("postgres://ardfard@127.0.0.1:5432/bunamateur_dev"),
        "postgres://ardfard@127.0.0.1:5432/bunamateur_dev"
    );
}

#[tokio::test]
async fn test_postgres_migrations_idempotent_seeding_and_api_lifecycle() {
    let pool = reset_and_connect_test_db().await;

    // 1. Run embedded migrations and initial seed
    run_migrations(&pool).await.expect("migrations should succeed");
    seed_content(&pool).await.expect("initial seed should succeed");

    let grammar_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM grammar_points")
        .fetch_one(&pool)
        .await
        .unwrap();
    let expected_sentences: usize = SEED_GRAMMAR.iter().map(|g| g.sentences.len()).sum();
    let sentence_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM grammar_sentences")
        .fetch_one(&pool)
        .await
        .unwrap();
    let passage_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM reading_passages")
        .fetch_one(&pool)
        .await
        .unwrap();
    let conj_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM conjugation_items")
        .fetch_one(&pool)
        .await
        .unwrap();
    let initial_log_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM review_logs")
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(grammar_count as usize, SEED_GRAMMAR.len());
    assert_eq!(sentence_count as usize, expected_sentences);
    assert_eq!(passage_count as usize, SEED_PASSAGES.len());
    assert_eq!(conj_count as usize, SEED_CONJUGATIONS.len());
    assert!(initial_log_count > 0);

    // 2. Mutate user progress, notes, and content title, then re-run seed_content to prove idempotent upsert without wiping user progress
    let first_gid: i64 =
        sqlx::query_scalar("SELECT id FROM grammar_points WHERE slug = 'te-iru'")
            .fetch_one(&pool)
            .await
            .unwrap();
    update_grammar_notes(&pool, first_gid, "Keep this personal study note!")
        .await
        .unwrap();
    sqlx::query("UPDATE user_progress SET srs_stage = 10, streak = 99 WHERE grammar_id = $1")
        .bind(first_gid)
        .execute(&pool)
        .await
        .unwrap();
    // Simulate stale curriculum text that should be refreshed by seed upsert
    sqlx::query("UPDATE grammar_points SET meaning = 'STALE MEANING' WHERE id = $1")
        .bind(first_gid)
        .execute(&pool)
        .await
        .unwrap();

    // Re-run migrations and seeding
    run_migrations(&pool).await.expect("re-running migrations should be idempotent");
    seed_content(&pool).await.expect("re-running seed_content should be idempotent");

    let grammar_count_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM grammar_points")
        .fetch_one(&pool)
        .await
        .unwrap();
    let sentence_count_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM grammar_sentences")
        .fetch_one(&pool)
        .await
        .unwrap();
    let log_count_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM review_logs")
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(grammar_count_after, grammar_count);
    assert_eq!(sentence_count_after, sentence_count);
    assert_eq!(log_count_after, initial_log_count, "review_logs must not duplicate on re-seed");

    let detail = get_grammar_detail(&pool, first_gid)
        .await
        .unwrap()
        .expect("te-iru grammar point should exist");
    assert_eq!(detail.meaning, "Ongoing action or resulting state", "curriculum content should be updated on upsert");
    assert_eq!(detail.user_notes, "Keep this personal study note!", "user notes must be preserved");
    assert_eq!(detail.srs_stage, 10, "user SRS stage must be preserved");
    assert_eq!(detail.streak, 99, "user streak must be preserved");

    // 3. Exercise DB queries directly
    let dashboard = compute_dashboard(&pool).await.unwrap();
    assert_eq!(dashboard.total_grammar as usize, SEED_GRAMMAR.len());
    assert_eq!(dashboard.jlpt_progress.len(), 5);
    assert_eq!(dashboard.srs_breakdown.len(), 5);
    assert_eq!(dashboard.forecast_24h.len(), 8);
    assert!(dashboard.streak_days >= 5);

    let n4_grammar = list_grammar_summaries(&pool, Some("N4"), None, None)
        .await
        .unwrap();
    assert!(!n4_grammar.is_empty());
    assert!(n4_grammar.iter().all(|g| g.jlpt_level == "N4"));

    let review_queue = build_review_queue(&pool, "review", None, 20).await.unwrap();
    assert!(!review_queue.is_empty());
    let lesson_queue = build_review_queue(&pool, "lesson", None, 20).await.unwrap();
    assert!(!lesson_queue.is_empty());
    let ghost_queue = build_review_queue(&pool, "ghost", None, 20).await.unwrap();
    assert!(!ghost_queue.is_empty());
    let cram_queue = build_review_queue(&pool, "cram", Some("N5"), 5).await.unwrap();
    assert!(!cram_queue.is_empty());
    assert!(cram_queue.iter().all(|i| i.jlpt_level == "N5"));

    let passages = list_reading_passages(&pool).await.unwrap();
    assert_eq!(passages.len(), SEED_PASSAGES.len());
    assert!(!passages[0].detected_grammar.is_empty());

    let detected = detect_grammar_in_text(&pool, "雨が降ったのに、ピクニックに行きました。")
        .await
        .unwrap();
    assert!(detected.iter().any(|d| d.title == "〜のに"));

    let te_conjs = list_conjugations(&pool, Some("te")).await.unwrap();
    assert!(!te_conjs.is_empty());
    assert!(te_conjs.iter().all(|c| c.target_form == "te"));

    let wk_initial = load_wanikani_status(&pool).await.unwrap();
    assert!(!wk_initial.connected);
    assert!(wk_initial.smart_furigana);

    // 4. Exercise HTTP API routes end-to-end against PostgreSQL
    let state = AppState::from_pool(pool.clone()).await.unwrap();
    let app = axum::Router::new()
        .nest("/api", routes::api_router())
        .with_state(state);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = reqwest::Client::new();
    let base = format!("http://{}/api", addr);

    // Dashboard endpoint
    let resp: Value = client
        .get(format!("{}/dashboard", base))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(resp["total_grammar"].as_u64(), Some(SEED_GRAMMAR.len() as u64));

    // WaniKani demo sync -> settings -> disconnect
    let wk_demo: Value = client
        .post(format!("{}/wanikani/demo", base))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(wk_demo["connected"], Value::Bool(true));
    assert_eq!(wk_demo["is_demo"], Value::Bool(true));
    assert!(wk_demo["kanji_known"].as_u64().unwrap() > 100);

    let wk_settings: Value = client
        .put(format!("{}/wanikani/settings", base))
        .json(&json!({ "smart_furigana": false, "highlight_known_kanji": true }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(wk_settings["smart_furigana"], Value::Bool(false));

    // Reading analyze with WaniKani synced
    let analyzed: Value = client
        .post(format!("{}/reading/analyze", base))
        .json(&json!({ "text": "毎日日本語を勉強しているおかげで、少しずつ新聞が読めるようになりました。" }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(analyzed["known_kanji"].as_u64().unwrap() > 0);
    assert!(!analyzed["detected_grammar"].as_array().unwrap().is_empty());

    // Submit a review answer (miss -> spawns ghost), then undo it
    let target_item = review_queue
        .iter()
        .find(|i| !i.is_ghost)
        .expect("should have a non-ghost due review");
    let submit_resp: Value = client
        .post(format!("{}/reviews/submit", base))
        .json(&json!({
            "grammar_id": target_item.grammar_id,
            "sentence_id": target_item.sentence_id,
            "user_input": "まちがい",
            "mode": "review",
            "is_ghost": false
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(submit_resp["correct"], Value::Bool(false));
    assert_eq!(submit_resp["new_ghost_level"].as_i64(), Some(3));
    let review_log_id = submit_resp["review_log_id"].as_i64().expect("review_log_id should be returned");

    client
        .post(format!("{}/reviews/undo", base))
        .json(&json!({
            "grammar_id": target_item.grammar_id,
            "previous_srs_stage": submit_resp["previous_srs_stage"],
            "previous_ghost_level": submit_resp["previous_ghost_level"],
            "review_log_id": review_log_id
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();

    let after_undo = get_grammar_detail(&pool, target_item.grammar_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        after_undo.srs_stage,
        submit_resp["previous_srs_stage"].as_i64().unwrap() as i32
    );
    assert_eq!(
        after_undo.ghost_level,
        submit_resp["previous_ghost_level"].as_i64().unwrap() as i32
    );

    // Start a lesson & reset progress
    let lesson_gid = lesson_queue[0].grammar_id;
    client
        .post(format!("{}/lessons/start", base))
        .json(&json!({ "grammar_id": lesson_gid }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    assert_eq!(
        get_grammar_detail(&pool, lesson_gid).await.unwrap().unwrap().srs_stage,
        1
    );

    client
        .post(format!("{}/grammar/{}/reset", base, lesson_gid))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    assert_eq!(
        get_grammar_detail(&pool, lesson_gid).await.unwrap().unwrap().srs_stage,
        0
    );

    // Disconnect WaniKani
    let wk_disc: Value = client
        .post(format!("{}/wanikani/disconnect", base))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(wk_disc["connected"], Value::Bool(false));
    assert_eq!(wk_disc["kanji_known"].as_u64(), Some(0));

    server.abort();
}

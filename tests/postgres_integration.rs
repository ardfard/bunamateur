use bunamateur::{
    crypto::TokenCipher,
    db::{
        AppState, build_review_queue, compute_dashboard, detect_grammar_in_text,
        get_grammar_detail, get_wanikani_credentials, list_conjugations, list_grammar_summaries,
        list_reading_passages, load_wanikani_status, run_migrations, sanitize_database_url,
        save_wanikani_sync, seed_content, update_grammar_notes,
    },
    routes,
    security::{RateLimitConfig, RateLimitRule, SecurityConfig, build_cors_layer},
    seed_data::{SEED_CONJUGATIONS, SEED_PASSAGES},
    seed_data_extra::{CONTENT_SCHEMA_VERSION, all_seed_grammar, sentences_for_grammar},
    wanikani::WaniKaniSyncResult,
};
use axum::http::{StatusCode, header};
use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::TcpListener;

const DEFAULT_TEST_DATABASE_URL: &str = "postgres://ardfard@127.0.0.1:5432/bunamateur_test";
const TEST_ENCRYPTION_KEY: &str =
    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

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

fn build_test_app(state: AppState) -> axum::Router {
    let cors = build_cors_layer(&state.security).expect("valid test CORS config");
    let api = routes::api_router(&state);
    let ops = routes::ops_router();
    let metrics = state.metrics.clone();
    axum::Router::new()
        .merge(ops)
        .nest("/api", api)
        .layer(axum::middleware::from_fn(move |req, next| {
            bunamateur::observability::metrics_middleware(metrics.clone(), req, next)
        }))
        .layer(cors)
        .with_state(state)
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
async fn test_multi_user_auth_csrf_encryption_cors_limits_and_isolation() {
    let pool = reset_and_connect_test_db().await;

    // 1. Run embedded migrations and idempotent curriculum seeding
    run_migrations(&pool).await.expect("migrations should succeed");
    seed_content(&pool).await.expect("initial seed should succeed");

    let expected_grammar = all_seed_grammar();
    let expected_sentences: usize = expected_grammar
        .iter()
        .map(|g| sentences_for_grammar(g).len())
        .sum();
    let grammar_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM grammar_points")
        .fetch_one(&pool)
        .await
        .unwrap();
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

    assert_eq!(grammar_count as usize, expected_grammar.len());
    assert!(grammar_count >= 50, "expanded curriculum must have 50+ grammar points");
    assert_eq!(sentence_count as usize, expected_sentences);
    assert!(sentence_count >= 300, "expanded curriculum must have 6+ sentences per point");
    assert_eq!(passage_count as usize, SEED_PASSAGES.len());
    assert_eq!(conj_count as usize, SEED_CONJUGATIONS.len());

    // 2. Start HTTP server with strict CORS, body limits, and deterministic rate limits
    let cipher = TokenCipher::from_key_str(TEST_ENCRYPTION_KEY).unwrap();
    let security = SecurityConfig {
        allowed_origins: vec!["https://app.bunamateur.test".to_string()],
        cookie_secure: false, // plain HTTP localhost test server
        session_ttl_days: 7,
        trust_proxy: false,
        rate_limits: RateLimitConfig {
            auth: RateLimitRule {
                max_requests: 6,
                window: Duration::from_secs(60),
            },
            wanikani: RateLimitRule {
                max_requests: 10,
                window: Duration::from_secs(60),
            },
            general: RateLimitRule {
                max_requests: 200,
                window: Duration::from_secs(60),
            },
        },
    };
    let state = AppState::from_pool_with_config(pool.clone(), cipher.clone(), security)
        .await
        .unwrap();
    let app = build_test_app(state);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });

    let base = format!("http://{}/api", addr);
    let anon = reqwest::Client::new();

    // Unauthenticated requests must be rejected with 401
    let unauth_dash = anon.get(format!("{}/dashboard", base)).send().await.unwrap();
    assert_eq!(unauth_dash.status(), StatusCode::UNAUTHORIZED);

    // CORS hardening: allowed origin receives ACAO + ACAC; disallowed origin does not
    let allowed_preflight = anon
        .request(reqwest::Method::OPTIONS, format!("{}/dashboard", base))
        .header(header::ORIGIN, "https://app.bunamateur.test")
        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
        .send()
        .await
        .unwrap();
    assert_eq!(
        allowed_preflight
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .and_then(|v| v.to_str().ok()),
        Some("https://app.bunamateur.test")
    );
    assert_eq!(
        allowed_preflight
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_CREDENTIALS)
            .and_then(|v| v.to_str().ok()),
        Some("true")
    );

    let blocked_preflight = anon
        .request(reqwest::Method::OPTIONS, format!("{}/dashboard", base))
        .header(header::ORIGIN, "https://evil.example")
        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
        .send()
        .await
        .unwrap();
    assert!(
        blocked_preflight
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .is_none(),
        "disallowed origin must not receive Access-Control-Allow-Origin"
    );

    // 3. Register User A & inspect cookie flags
    let client_a = reqwest::Client::builder().cookie_store(true).build().unwrap();
    let reg_a_raw = client_a
        .post(format!("{}/auth/register", base))
        .json(&json!({
            "email": "alice@example.com",
            "username": "alice_jp",
            "password": "super-secret-password-1"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(reg_a_raw.status(), StatusCode::CREATED);
    let set_cookies: Vec<String> = reg_a_raw
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok().map(|s| s.to_string()))
        .collect();
    assert!(
        set_cookies
            .iter()
            .any(|c| c.starts_with("bunamateur_session=") && c.contains("HttpOnly") && c.contains("SameSite=Lax")),
        "session cookie must set HttpOnly and SameSite=Lax: {:?}",
        set_cookies
    );
    let reg_a: Value = reg_a_raw.json().await.unwrap();
    let user_a_id = reg_a["user"]["id"].as_i64().unwrap();
    let csrf_a = reg_a["csrf_token"].as_str().unwrap().to_string();
    assert_eq!(reg_a["user"]["email_verified"], Value::Bool(false));

    // Duplicate email rejected with 409
    let dup = anon
        .post(format!("{}/auth/register", base))
        .json(&json!({
            "email": "ALICE@example.com",
            "username": "other_name",
            "password": "super-secret-password-2"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(dup.status(), StatusCode::CONFLICT);

    // Register User B
    let client_b = reqwest::Client::builder().cookie_store(true).build().unwrap();
    let reg_b: Value = client_b
        .post(format!("{}/auth/register", base))
        .json(&json!({
            "email": "bob@example.com",
            "username": "bob_jp",
            "password": "super-secret-password-2"
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let user_b_id = reg_b["user"]["id"].as_i64().unwrap();
    let csrf_b = reg_b["csrf_token"].as_str().unwrap().to_string();
    assert_ne!(user_a_id, user_b_id);
    assert_eq!(reg_b["user"]["email_verified"], Value::Bool(false));

    // 4. Verify CSRF protection on mutating endpoints & removal of demo endpoint
    let missing_csrf = client_a
        .put(format!("{}/wanikani/settings", base))
        .json(&json!({
            "smart_furigana": true,
            "highlight_known_kanji": true
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(missing_csrf.status(), StatusCode::FORBIDDEN);

    let wrong_csrf = client_a
        .put(format!("{}/wanikani/settings", base))
        .header("X-CSRF-Token", "not-the-real-token")
        .json(&json!({
            "smart_furigana": true,
            "highlight_known_kanji": true
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(wrong_csrf.status(), StatusCode::FORBIDDEN);

    let valid_settings = client_a
        .put(format!("{}/wanikani/settings", base))
        .header("X-CSRF-Token", &csrf_a)
        .json(&json!({
            "smart_furigana": true,
            "highlight_known_kanji": false
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(valid_settings.status(), StatusCode::OK);
    let wk_settings_json: Value = valid_settings.json().await.unwrap();
    assert_eq!(wk_settings_json["smart_furigana"], Value::Bool(true));
    assert_eq!(wk_settings_json["highlight_known_kanji"], Value::Bool(false));

    let removed_demo = client_a
        .post(format!("{}/wanikani/demo", base))
        .header("X-CSRF-Token", &csrf_a)
        .send()
        .await
        .unwrap();
    assert_eq!(removed_demo.status(), StatusCode::NOT_FOUND);

    // 5. Verify WaniKani api_token is encrypted at rest with AES-256-GCM + user_id AAD
    //    and multi-user isolation holds
    let sample_sync = WaniKaniSyncResult {
        username: "alice_wk".to_string(),
        level: 10,
        lessons_available: 4,
        reviews_available: 12,
        subjects: vec![
            (1001, "kanji".to_string(), "日".to_string(), 2, 8),
            (1002, "kanji".to_string(), "本".to_string(), 2, 8),
            (1003, "kanji".to_string(), "語".to_string(), 10, 6),
            (1004, "kanji".to_string(), "学".to_string(), 5, 7),
            (1005, "kanji".to_string(), "生".to_string(), 3, 8),
        ],
        removed_subject_ids: Vec::new(),
        is_incremental: false,
        user_etag: None,
        summary_etag: None,
        assignments_updated_after: None,
        assignments_etag: None,
        subjects_updated_after: None,
        subjects_etag: None,
    };
    let secret_wk_token = "wk-v2-live-secret-token-xyz987";
    let wk_a = save_wanikani_sync(
        &pool,
        &cipher,
        user_a_id,
        Some(secret_wk_token),
        &sample_sync,
    )
    .await
    .unwrap();
    assert!(wk_a.connected);
    assert_eq!(wk_a.kanji_known, 5);

    // User B must remain disconnected and have 0 synced kanji (multi-user isolation!)
    let wk_b = load_wanikani_status(&pool, user_b_id).await.unwrap();
    assert!(!wk_b.connected);
    assert_eq!(wk_b.kanji_known, 0);

    let raw_stored: String =
        sqlx::query_scalar("SELECT api_token FROM wk_profile WHERE user_id = $1")
            .bind(user_a_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(raw_stored.starts_with("enc:v1:"));
    assert!(
        !raw_stored.contains(secret_wk_token),
        "plaintext WaniKani token must never be stored in DB"
    );

    let decrypted = get_wanikani_credentials(&pool, &cipher, user_a_id)
        .await
        .unwrap();
    assert_eq!(decrypted.as_deref(), Some(secret_wk_token));
    assert!(
        cipher.decrypt_for_user(user_b_id, &raw_stored).is_err(),
        "encrypted token must be bound to owning user_id via AAD"
    );

    // 6. Verify multi-user progress, notes, review submission & undo isolation
    let first_gid: i64 = sqlx::query_scalar("SELECT id FROM grammar_points WHERE slug = 'te-iru'")
        .fetch_one(&pool)
        .await
        .unwrap();

    // User A updates notes & starts lesson
    client_a
        .put(format!("{}/grammar/{}/notes", base, first_gid))
        .header("X-CSRF-Token", &csrf_a)
        .json(&json!({ "notes": "Alice private note" }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();

    client_a
        .post(format!("{}/lessons/start", base))
        .header("X-CSRF-Token", &csrf_a)
        .json(&json!({ "grammar_id": first_gid }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();

    // Re-running seed_content must preserve User A's notes & progress while keeping curriculum fresh
    sqlx::query("UPDATE grammar_points SET meaning = 'STALE' WHERE id = $1")
        .bind(first_gid)
        .execute(&pool)
        .await
        .unwrap();
    seed_content(&pool).await.unwrap();

    let detail_a = get_grammar_detail(&pool, user_a_id, first_gid)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(detail_a.meaning, "Ongoing action or resulting state");
    assert_eq!(detail_a.user_notes, "Alice private note");
    assert_eq!(detail_a.srs_stage, 1);

    let detail_b = get_grammar_detail(&pool, user_b_id, first_gid)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(detail_b.user_notes, "");
    assert_eq!(detail_b.srs_stage, 0);

    // User A submits a wrong review -> spawns ghost; User B cannot undo User A's review log
    let sentence_id = detail_a.sentences[0].id;
    let submit_a: Value = client_a
        .post(format!("{}/reviews/submit", base))
        .header("X-CSRF-Token", &csrf_a)
        .json(&json!({
            "grammar_id": first_gid,
            "sentence_id": sentence_id,
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
    assert_eq!(submit_a["correct"], Value::Bool(false));
    assert_eq!(submit_a["new_ghost_level"].as_i64(), Some(3));
    let log_a_id = submit_a["review_log_id"].as_i64().unwrap();

    // User B tries to undo using User A's review_log_id -> must NOT delete User A's log
    client_b
        .post(format!("{}/reviews/undo", base))
        .header("X-CSRF-Token", &csrf_b)
        .json(&json!({
            "grammar_id": first_gid,
            "previous_srs_stage": 0,
            "previous_ghost_level": 0,
            "review_log_id": log_a_id
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();

    let log_still_exists: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM review_logs WHERE id = $1 AND user_id = $2")
            .bind(log_a_id)
            .bind(user_a_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(log_still_exists, 1, "User B must not delete User A's review log");

    // User A undoes own review -> log deleted & stage restored
    client_a
        .post(format!("{}/reviews/undo", base))
        .header("X-CSRF-Token", &csrf_a)
        .json(&json!({
            "grammar_id": first_gid,
            "previous_srs_stage": submit_a["previous_srs_stage"],
            "previous_ghost_level": submit_a["previous_ghost_level"],
            "review_log_id": log_a_id
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let after_undo_a = get_grammar_detail(&pool, user_a_id, first_gid)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(after_undo_a.srs_stage, 1);
    assert_eq!(after_undo_a.ghost_level, 0);

    // 7. Exercise direct DB queries scoped per user
    let dash_a = compute_dashboard(&pool, user_a_id).await.unwrap();
    let dash_b = compute_dashboard(&pool, user_b_id).await.unwrap();
    assert_eq!(dash_a.total_learned, 1);
    assert_eq!(dash_b.total_learned, 0);
    assert_eq!(dash_b.lessons_available as usize, expected_grammar.len());
    assert!(!dash_b.onboarding.completed);

    let n4_grammar = list_grammar_summaries(&pool, user_a_id, Some("N4"), None, None, None)
        .await
        .unwrap();
    assert!(!n4_grammar.is_empty());

    // 7b. Verify content version, zero-state onboarding, contextual nuance grading,
    //     Vacation Mode, Ghost Intensity, and JSON/CSV progress export + import
    let content_ver: Value = client_a
        .get(format!("{}/content/version", base))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(content_ver["version"].as_i64(), Some(CONTENT_SCHEMA_VERSION as i64));
    assert_eq!(content_ver["item_count"].as_i64(), Some(expected_grammar.len() as i64));

    let placement: Value = client_b
        .get(format!("{}/onboarding/placement", base))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let placement_items = placement.as_array().unwrap();
    assert_eq!(placement_items.len(), 12);
    let placement_levels: Vec<&str> = placement_items
        .iter()
        .map(|item| item["jlpt_level"].as_str().unwrap())
        .collect();
    for required_level in ["N5", "N4", "N3", "N2", "N1"] {
        assert!(
            placement_levels.contains(&required_level),
            "placement questions must include {required_level}: {placement_levels:?}"
        );
    }

    let onboarding_done: Value = client_b
        .post(format!("{}/onboarding/complete", base))
        .header("X-CSRF-Token", &csrf_b)
        .json(&json!({
            "study_path": "Tobira",
            "placement_jlpt": "N3",
            "daily_goal": 30,
            "known_grammar_ids": [first_gid]
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(onboarding_done["completed"], Value::Bool(true));
    assert_eq!(onboarding_done["preferred_study_path"], "Tobira");
    assert_eq!(
        get_grammar_detail(&pool, user_b_id, first_gid).await.unwrap().unwrap().srs_stage,
        5
    );

    // Reset onboarding with valid CSRF -> completed becomes false while preserving prefs
    let onboarding_reset_missing_csrf = client_b
        .post(format!("{}/onboarding/reset", base))
        .send()
        .await
        .unwrap();
    assert_eq!(onboarding_reset_missing_csrf.status(), StatusCode::FORBIDDEN);

    let onboarding_reset: Value = client_b
        .post(format!("{}/onboarding/reset", base))
        .header("X-CSRF-Token", &csrf_b)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(onboarding_reset["completed"], Value::Bool(false));
    assert_eq!(onboarding_reset["preferred_study_path"], "Tobira");
    assert_eq!(onboarding_reset["placement_jlpt"], "N3");
    assert_eq!(onboarding_reset["daily_goal"], 30);

    // Re-completing onboarding restores completed: true
    let onboarding_recompleted: Value = client_b
        .post(format!("{}/onboarding/complete", base))
        .header("X-CSRF-Token", &csrf_b)
        .json(&json!({
            "study_path": "Tobira",
            "placement_jlpt": "N3",
            "daily_goal": 30,
            "known_grammar_ids": [first_gid]
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(onboarding_recompleted["completed"], Value::Bool(true));
    assert_eq!(onboarding_recompleted["preferred_study_path"], "Tobira");

    // Contextual nuance grading: wrong tense on te-iru sentence[0] (present expected, past typed) -> almost_right, no SRS penalty
    let almost_resp: Value = client_a
        .post(format!("{}/reviews/submit", base))
        .header("X-CSRF-Token", &csrf_a)
        .json(&json!({
            "grammar_id": first_gid,
            "sentence_id": sentence_id,
            "user_input": "ていました",
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
    assert_eq!(almost_resp["almost_right"], Value::Bool(true));
    assert_eq!(almost_resp["nuance_category"], "tense");
    assert!(almost_resp["review_log_id"].is_null());
    assert_eq!(
        get_grammar_detail(&pool, user_a_id, first_gid).await.unwrap().unwrap().ghost_level,
        0,
        "almost_right must not spawn a ghost or penalize SRS"
    );

    // Vacation Mode & Ghost Intensity settings
    let vac_on: Value = client_a
        .put(format!("{}/settings/srs", base))
        .header("X-CSRF-Token", &csrf_a)
        .json(&json!({ "vacation_mode": true, "ghost_intensity": "minimal" }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(vac_on["vacation_mode"], Value::Bool(true));
    assert_eq!(vac_on["ghost_intensity"], "minimal");
    assert!(vac_on["vacation_started_at"].is_string());

    let vac_dash = compute_dashboard(&pool, user_a_id).await.unwrap();
    assert_eq!(vac_dash.reviews_due, 0);

    let vac_off: Value = client_a
        .put(format!("{}/settings/srs", base))
        .header("X-CSRF-Token", &csrf_a)
        .json(&json!({ "vacation_mode": false }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(vac_off["vacation_mode"], Value::Bool(false));

    // Export JSON + CSV from User A and import CSV into User B
    let exported_json = client_a
        .get(format!("{}/export/progress?format=json", base))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(exported_json.contains("te-iru"));

    let exported_csv = client_a
        .get(format!("{}/export/progress?format=csv", base))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(exported_csv.contains("slug,title,jlpt_level"));
    assert!(exported_csv.contains("te-iru"));

    let import_res: Value = client_b
        .post(format!("{}/import/progress", base))
        .header("X-CSRF-Token", &csrf_b)
        .json(&json!({ "format": "csv", "data": exported_csv }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(import_res["imported"].as_u64().unwrap() >= 1);
    assert_eq!(
        get_grammar_detail(&pool, user_b_id, first_gid).await.unwrap().unwrap().user_notes,
        "Alice private note"
    );
    let lesson_q = build_review_queue(&pool, user_b_id, "lesson", None, 20)
        .await
        .unwrap();
    assert!(!lesson_q.is_empty());
    let passages = list_reading_passages(&pool, user_a_id).await.unwrap();
    assert_eq!(passages.len(), SEED_PASSAGES.len());
    let detected = detect_grammar_in_text(&pool, user_a_id, "雨が降ったのに、ピクニックに行きました。")
        .await
        .unwrap();
    assert!(detected.iter().any(|d| d.title == "〜のに"));
    let te_conjs = list_conjugations(&pool, Some("te")).await.unwrap();
    assert!(!te_conjs.is_empty());
    update_grammar_notes(&pool, user_b_id, first_gid, "Bob note")
        .await
        .unwrap();
    assert_eq!(
        get_grammar_detail(&pool, user_a_id, first_gid)
            .await
            .unwrap()
            .unwrap()
            .user_notes,
        "Alice private note"
    );

    // 8. Request body limit enforcement (413 Payload Too Large)
    let huge_notes = "あ".repeat(80 * 1024); // > 64 KiB default limit
    let oversized = client_a
        .put(format!("{}/grammar/{}/notes", base, first_gid))
        .header("X-CSRF-Token", &csrf_a)
        .json(&json!({ "notes": huge_notes }))
        .send()
        .await
        .unwrap();
    assert_eq!(oversized.status(), StatusCode::PAYLOAD_TOO_LARGE);

    // 9. Logout invalidates session cookie
    client_a
        .post(format!("{}/auth/logout", base))
        .header("X-CSRF-Token", &csrf_a)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let after_logout = client_a.get(format!("{}/auth/me", base)).send().await.unwrap();
    assert_eq!(after_logout.status(), StatusCode::UNAUTHORIZED);

    // 10. Per-route Auth rate limiting (max_requests = 6; we already made 3 auth calls above)
    let mut got_429 = false;
    for _ in 0..6 {
        let r = anon
            .post(format!("{}/auth/login", base))
            .json(&json!({ "email": "alice@example.com", "password": "wrong-password" }))
            .send()
            .await
            .unwrap();
        if r.status() == StatusCode::TOO_MANY_REQUESTS {
            assert!(r.headers().get(header::RETRY_AFTER).is_some());
            got_429 = true;
            break;
        }
    }
    assert!(got_429, "auth route must enforce rate limit with 429 Too Many Requests");

    // General tier remains available even after Auth tier is exhausted
    let still_ok = client_b.get(format!("{}/auth/me", base)).send().await.unwrap();
    assert_eq!(still_ok.status(), StatusCode::OK);

    // 11. Google OAuth 2.0 + PKCE S256 login & account linking end-to-end against mock Google server
    use axum::{Form, Json, Router, routing::{get, post}};
    use std::collections::HashMap;
    use bunamateur::oauth::GoogleOAuthConfig;

    let mock_google = Router::new()
        .route(
            "/token",
            post(|Form(params): Form<HashMap<String, String>>| async move {
                assert_eq!(params.get("grant_type").map(String::as_str), Some("authorization_code"));
                assert!(params.get("code_verifier").map(|v| v.len() >= 43).unwrap_or(false));
                let tok = match params.get("code").map(String::as_str) {
                    Some("code-new-user") => "access-new-user",
                    Some("code-existing-bob") => "access-bob",
                    _ => "access-invalid",
                };
                (StatusCode::OK, Json(json!({ "access_token": tok })))
            }),
        )
        .route(
            "/userinfo",
            get(|headers: axum::http::HeaderMap| async move {
                let auth = headers
                    .get(header::AUTHORIZATION)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");
                if auth == "Bearer access-bob" {
                    return Json(json!({
                        "sub": "google-sub-bob-99",
                        "email": "bob@example.com",
                        "email_verified": true,
                        "name": "Bob Learner",
                        "picture": "https://lh3.googleusercontent.com/bob.png"
                    }));
                }
                Json(json!({
                    "sub": "google-sub-hanako-77",
                    "email": "hanako@gmail.com",
                    "email_verified": true,
                    "name": "Hanako Yamada",
                    "picture": "https://lh3.googleusercontent.com/hanako.png"
                }))
            }),
        );

    let g_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let g_addr = g_listener.local_addr().unwrap();
    let g_server = tokio::spawn(async move {
        axum::serve(g_listener, mock_google).await.unwrap();
    });

    let oauth_security = SecurityConfig {
        allowed_origins: vec!["https://app.bunamateur.test".to_string()],
        cookie_secure: false,
        session_ttl_days: 7,
        trust_proxy: false,
        rate_limits: RateLimitConfig {
            auth: RateLimitRule {
                max_requests: 30,
                window: Duration::from_secs(60),
            },
            ..RateLimitConfig::default()
        },
    };
    let oauth_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let oauth_addr = oauth_listener.local_addr().unwrap();
    let google_cfg = GoogleOAuthConfig {
        client_id: Some("mock-google-client-id".to_string()),
        client_secret: Some("mock-google-secret".to_string()),
        redirect_uri: format!("http://{}/api/auth/google/callback", oauth_addr),
        auth_url: format!("http://{}/authorize", g_addr),
        token_url: format!("http://{}/token", g_addr),
        userinfo_url: format!("http://{}/userinfo", g_addr),
    };
    let oauth_state = AppState::from_pool_with_config(pool.clone(), cipher.clone(), oauth_security)
        .await
        .unwrap()
        .with_google_oauth(google_cfg);
    let oauth_app = build_test_app(oauth_state);
    let oauth_server = tokio::spawn(async move {
        axum::serve(
            oauth_listener,
            oauth_app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });

    let oauth_base = format!("http://{}/api", oauth_addr);
    let no_redirect_client = reqwest::Client::builder()
        .cookie_store(true)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    // Providers endpoint reports google: true
    let providers: Value = no_redirect_client
        .get(format!("{}/auth/providers", oauth_base))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(providers["google"], Value::Bool(true));

    // Start Google OAuth -> 307 redirect to Google with state + PKCE S256 + state cookie
    let start_resp = no_redirect_client
        .get(format!("{}/auth/google/start", oauth_base))
        .send()
        .await
        .unwrap();
    assert!(start_resp.status().is_redirection());
    let location = start_resp
        .headers()
        .get(header::LOCATION)
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(location.contains("code_challenge_method=S256"));
    let parsed_url = reqwest::Url::parse(&location).unwrap();
    let state_param = parsed_url
        .query_pairs()
        .find(|(k, _)| k == "state")
        .map(|(_, v)| v.to_string())
        .unwrap();

    // Callback with wrong state is rejected
    let bad_cb = no_redirect_client
        .get(format!(
            "{}/auth/google/callback?code=code-new-user&state=tampered",
            oauth_base
        ))
        .send()
        .await
        .unwrap();
    assert!(bad_cb.status().is_redirection());
    assert_eq!(
        bad_cb.headers().get(header::LOCATION).unwrap().to_str().unwrap(),
        "/?auth_error=invalid_state"
    );

    // Re-start to get fresh state cookie, then complete valid callback for brand-new Google user
    let start_resp2 = no_redirect_client
        .get(format!("{}/auth/google/start", oauth_base))
        .send()
        .await
        .unwrap();
    let loc2 = start_resp2
        .headers()
        .get(header::LOCATION)
        .unwrap()
        .to_str()
        .unwrap();
    let state_param2 = reqwest::Url::parse(loc2)
        .unwrap()
        .query_pairs()
        .find(|(k, _)| k == "state")
        .map(|(_, v)| v.to_string())
        .unwrap();
    assert_ne!(state_param, state_param2);

    let ok_cb = no_redirect_client
        .get(format!(
            "{}/auth/google/callback?code=code-new-user&state={}",
            oauth_base, state_param2
        ))
        .send()
        .await
        .unwrap();
    assert!(ok_cb.status().is_redirection());
    assert_eq!(
        ok_cb.headers().get(header::LOCATION).unwrap().to_str().unwrap(),
        "/"
    );

    // Session cookie issued by Google callback works immediately on /api/auth/me
    let me_google: Value = no_redirect_client
        .get(format!("{}/auth/me", oauth_base))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(me_google["user"]["email"], "hanako@gmail.com");
    assert_eq!(me_google["user"]["username"], "hanako_yamada");
    assert_eq!(me_google["user"]["email_verified"], Value::Bool(true));
    assert_eq!(
        me_google["user"]["avatar_url"],
        "https://lh3.googleusercontent.com/hanako.png"
    );

    // Account linking: Google login with Bob's verified email links to existing User B id!
    let bob_oauth_client = reqwest::Client::builder()
        .cookie_store(true)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let bob_start = bob_oauth_client
        .get(format!("{}/auth/google/start", oauth_base))
        .send()
        .await
        .unwrap();
    let bob_state = reqwest::Url::parse(
        bob_start.headers().get(header::LOCATION).unwrap().to_str().unwrap(),
    )
    .unwrap()
    .query_pairs()
    .find(|(k, _)| k == "state")
    .map(|(_, v)| v.to_string())
    .unwrap();
    let bob_cb = bob_oauth_client
        .get(format!(
            "{}/auth/google/callback?code=code-existing-bob&state={}",
            oauth_base, bob_state
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(
        bob_cb.headers().get(header::LOCATION).unwrap().to_str().unwrap(),
        "/"
    );
    let bob_me: Value = bob_oauth_client
        .get(format!("{}/auth/me", oauth_base))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        bob_me["user"]["id"].as_i64(),
        Some(user_b_id),
        "verified Google email must link to existing user account"
    );
    assert_eq!(
        bob_me["user"]["email_verified"],
        Value::Bool(true),
        "linking a verified Google account must mark user email verified"
    );

    // 13. P2 Observability (/healthz, /readyz, /metrics), Pure-Rust NLP, and Audio endpoints
    let root_base = format!("http://{}", addr);
    let health_resp: Value = client_b
        .get(format!("{}/healthz", root_base))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(health_resp["status"], "ok");

    let ready_resp: Value = client_b
        .get(format!("{}/readyz", root_base))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(ready_resp["status"], "ready");
    assert_eq!(ready_resp["database"], "ok");

    // NLP reading analysis returns tokens, auto-furigana ruby, and lemmatized grammar hits
    let nlp_resp: Value = client_b
        .post(format!("{}/reading/analyze", base))
        .header("X-CSRF-Token", &csrf_b)
        .json(&json!({
            "text": "毎日日本語を勉強しているおかげで、少しずつ日本の新聞が読めるようになりました。"
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(
        !nlp_resp["tokens"].as_array().unwrap().is_empty(),
        "NLP analysis must return interactive tokens"
    );
    assert!(
        nlp_resp["annotated_ruby"]
            .as_str()
            .unwrap()
            .contains("[毎日|まいにち]")
    );
    assert!(!nlp_resp["detected_grammar"].as_array().unwrap().is_empty());

    // Sentence audio returns 404 when no cached/generated asset is present (triggering client fallback)
    let first_sentence_id: i64 = sqlx::query_scalar("SELECT id FROM grammar_sentences ORDER BY id LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    let audio_miss = client_b
        .get(format!("{}/audio/sentence/{}", base, first_sentence_id))
        .send()
        .await
        .unwrap();
    assert_eq!(audio_miss.status(), StatusCode::NOT_FOUND);

    // /metrics exposes Prometheus counters including recorded HTTP requests & NLP analyses
    let metrics_text = client_b
        .get(format!("{}/metrics", root_base))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(metrics_text.contains("http_requests_total"));
    assert!(metrics_text.contains("bunamateur_nlp_analyses_total"));
    assert!(metrics_text.contains("bunamateur_audio_requests_total"));

    // 14. v0.2 Account Security, Study Analytics & Reading Lab Personal Vocab Deck
    // Account profile + username update
    let profile: Value = client_b
        .get(format!("{}/account/profile", base))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(profile["has_password"], true);
    assert!(profile["active_sessions"].as_u64().unwrap() >= 1);

    let updated_profile: Value = client_b
        .put(format!("{}/account/profile", base))
        .header("X-CSRF-Token", &csrf_b)
        .json(&json!({ "username": "bob_renamed" }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(updated_profile["user"]["username"], "bob_renamed");

    // Active sessions list + revoke others
    let sessions: Vec<Value> = client_b
        .get(format!("{}/account/sessions", base))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(sessions.iter().any(|s| s["is_current"] == true));

    let revoke_others: Value = client_b
        .post(format!("{}/account/sessions/revoke-others", base))
        .header("X-CSRF-Token", &csrf_b)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(revoke_others["ok"], true);

    // Forgot-password never enumerates emails (returns 200 for unknown email; uses oauth_base whose auth bucket wasn't exhausted in step 6)
    let forgot_unknown: Value = client_b
        .post(format!("{}/auth/forgot-password", oauth_base))
        .json(&json!({ "email": "nonexistent@example.com" }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(forgot_unknown["ok"], true);

    // Password reset token consumption updates password and invalidates old sessions
    let raw_reset_token = bunamateur::auth::generate_random_token();
    let reset_hash = bunamateur::auth::hash_session_token(&raw_reset_token);
    bunamateur::db::create_password_reset_token(
        &pool,
        user_a_id,
        &reset_hash,
        chrono::Utc::now() + chrono::Duration::minutes(30),
    )
    .await
    .unwrap();
    let reset_ok: Value = client_a
        .post(format!("{}/auth/reset-password", oauth_base))
        .json(&json!({
            "token": raw_reset_token,
            "new_password": "new-super-secret-pass-99"
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(reset_ok["ok"], true);

    // Reusing the same reset token must fail with 400
    let reset_reuse = client_a
        .post(format!("{}/auth/reset-password", oauth_base))
        .json(&json!({
            "token": raw_reset_token,
            "new_password": "another-secret-pass-99"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(reset_reuse.status(), StatusCode::BAD_REQUEST);

    // 14b. Soft Email Verification end-to-end flow for User A (unverified after registration)
    let login_a2: Value = client_a
        .post(format!("{}/auth/login", oauth_base))
        .json(&json!({
            "email": "alice@example.com",
            "password": "new-super-secret-pass-99"
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(login_a2["user"]["email_verified"], Value::Bool(false));
    let csrf_a2 = login_a2["csrf_token"].as_str().unwrap().to_string();

    // Resend verification returns 200 and creates a fresh verification token
    let resend_resp: Value = client_a
        .post(format!("{}/auth/resend-verification", oauth_base))
        .header("X-CSRF-Token", &csrf_a2)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(resend_resp["ok"], Value::Bool(true));
    assert_eq!(resend_resp["already_verified"], Value::Bool(false));

    // Create a known verification token, consume via POST /api/auth/verify-email
    let raw_verify_token = bunamateur::auth::generate_random_token();
    let verify_hash = bunamateur::auth::hash_session_token(&raw_verify_token);
    bunamateur::db::create_email_verification_token(
        &pool,
        user_a_id,
        &verify_hash,
        chrono::Utc::now() + chrono::Duration::hours(24),
    )
    .await
    .unwrap();

    let verify_ok: Value = anon
        .post(format!("{}/auth/verify-email", oauth_base))
        .json(&json!({ "token": raw_verify_token }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(verify_ok["ok"], Value::Bool(true));

    // /api/auth/me now reports email_verified == true
    let me_a_verified: Value = client_a
        .get(format!("{}/auth/me", oauth_base))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(me_a_verified["user"]["email_verified"], Value::Bool(true));

    // Resending after verification reports already_verified == true
    let resend_already: Value = client_a
        .post(format!("{}/auth/resend-verification", oauth_base))
        .header("X-CSRF-Token", &csrf_a2)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(resend_already["ok"], Value::Bool(true));
    assert_eq!(resend_already["already_verified"], Value::Bool(true));

    // Reusing the same verification token must fail with 400
    let verify_reuse = anon
        .post(format!("{}/auth/verify-email", oauth_base))
        .json(&json!({ "token": raw_verify_token }))
        .send()
        .await
        .unwrap();
    assert_eq!(verify_reuse.status(), StatusCode::BAD_REQUEST);

    // Study analytics returns 91-day heatmap and JLPT N5-N1 rows
    let analytics: Value = client_b
        .get(format!("{}/analytics/study?days=91", base))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(analytics["days"], 91);
    assert_eq!(analytics["heatmap"].as_array().unwrap().len(), 91);
    assert_eq!(analytics["accuracy_by_jlpt"].as_array().unwrap().len(), 5);

    // Reading Lab personal vocabulary deck: save, list, Anki TSV export, delete
    let saved_vocab: Value = client_b
        .post(format!("{}/vocab/bookmarks", base))
        .header("X-CSRF-Token", &csrf_b)
        .json(&json!({
            "surface": "読める",
            "lemma": "読む",
            "reading": "よむ",
            "pos": "verb",
            "glosses": ["to read"],
            "jlpt": "N5",
            "wk_level": 10,
            "context_sentence": "新聞が読めるようになりました。"
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let bookmark_id = saved_vocab["id"].as_i64().unwrap();
    assert_eq!(saved_vocab["lemma"], "読む");

    let anki_tsv = client_b
        .get(format!("{}/vocab/bookmarks/export?format=anki", base))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(anki_tsv.contains("#separator:tab"));
    assert!(anki_tsv.contains("読む\tよむ\tto read"));

    let del_vocab: Value = client_b
        .delete(format!("{}/vocab/bookmarks/{}", base, bookmark_id))
        .header("X-CSRF-Token", &csrf_b)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(del_vocab["ok"], true);

    oauth_server.abort();
    g_server.abort();
    server.abort();
}

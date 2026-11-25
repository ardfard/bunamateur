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
    wanikani::generate_demo_wanikani_sync,
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
    axum::Router::new()
        .nest("/api", api)
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

    // 4. Verify CSRF protection on mutating endpoints
    let missing_csrf = client_a
        .post(format!("{}/wanikani/demo", base))
        .send()
        .await
        .unwrap();
    assert_eq!(missing_csrf.status(), StatusCode::FORBIDDEN);

    let wrong_csrf = client_a
        .post(format!("{}/wanikani/demo", base))
        .header("X-CSRF-Token", "not-the-real-token")
        .send()
        .await
        .unwrap();
    assert_eq!(wrong_csrf.status(), StatusCode::FORBIDDEN);

    // With valid CSRF token, User A activates WaniKani demo
    let wk_a: Value = client_a
        .post(format!("{}/wanikani/demo", base))
        .header("X-CSRF-Token", &csrf_a)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(wk_a["connected"], Value::Bool(true));
    assert!(wk_a["kanji_known"].as_u64().unwrap() > 100);

    // User B must remain disconnected and have 0 synced kanji (multi-user isolation!)
    let wk_b = load_wanikani_status(&pool, user_b_id).await.unwrap();
    assert!(!wk_b.connected);
    assert_eq!(wk_b.kanji_known, 0);

    // 5. Verify WaniKani api_token is encrypted at rest with AES-256-GCM + user_id AAD
    let demo_sync = generate_demo_wanikani_sync();
    let secret_wk_token = "wk-v2-live-secret-token-xyz987";
    save_wanikani_sync(
        &pool,
        &cipher,
        user_a_id,
        Some(secret_wk_token),
        false,
        &demo_sync,
    )
    .await
    .unwrap();

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

    let (decrypted, is_demo) = get_wanikani_credentials(&pool, &cipher, user_a_id)
        .await
        .unwrap();
    assert_eq!(decrypted.as_deref(), Some(secret_wk_token));
    assert!(!is_demo);
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
    assert!(placement.as_array().unwrap().len() >= 8);

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

    oauth_server.abort();
    g_server.abort();
    server.abort();
}

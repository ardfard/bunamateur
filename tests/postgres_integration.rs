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
    seed_data::{SEED_CONJUGATIONS, SEED_GRAMMAR, SEED_PASSAGES},
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

    assert_eq!(grammar_count as usize, SEED_GRAMMAR.len());
    assert_eq!(sentence_count as usize, expected_sentences);
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
    assert_eq!(dash_b.lessons_available as usize, SEED_GRAMMAR.len());

    let n4_grammar = list_grammar_summaries(&pool, user_a_id, Some("N4"), None, None)
        .await
        .unwrap();
    assert!(!n4_grammar.is_empty());
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

    server.abort();
}

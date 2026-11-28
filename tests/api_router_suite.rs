//! Offline integration tests using `tower::ServiceExt::oneshot` (no TCP sockets or live Postgres required).
//! Verifies HTTP routing, observability (`/healthz`, `/metrics`, Prometheus middleware),
//! static asset Cache-Control / ETag / 304, CORS & security layers, pure-Rust NLP + JMdict,
//! and pre-generated sentence audio resolution.

use axum::{
    Json, Router,
    body::Body,
    http::{HeaderMap, HeaderValue, Method, Request, StatusCode, header},
    middleware,
    response::IntoResponse,
    routing::{get, post},
};
use bunamateur::{
    audio::{AudioConfig, AudioProviderKind, AudioResolution, AudioService, compute_audio_hash},
    crypto::TokenCipher,
    db::{compute_longest_streak, review_intensity_bucket, vocab_bookmarks_to_anki_tsv},
    email::{build_email_verification_email, build_password_reset_email},
    lesson::{load_embedded_lessons, parse_lesson_json, validate_furigana},
    models::{GrammarPointDetail, ReviewQueueItem, VocabBookmark},
    nlp::{WkSubjectLookup, analyze_japanese_text, match_pattern_against_analysis},
    observability::{
        MetricsRegistry, PROMETHEUS_CONTENT_TYPE, accepts_gzip, compute_weak_etag, gzip_compress,
        is_compressible_mime, matches_if_none_match, metrics_middleware, static_cache_control,
    },
    security::{
        AUTH_BODY_LIMIT_BYTES, RateLimitConfig, RateLimitRule, RateLimitTier, RateLimiter,
        SecurityConfig, build_cors_layer, rate_limit_middleware,
    },
    seed_data_extra::all_seed_grammar,
    srs::{GhostIntensity, GradeOutcome, compute_srs_transition_with_intensity, grade_answer},
};
use chrono::Utc;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::time::Duration;
use tower::ServiceExt;

#[tokio::test]
async fn test_tower_oneshot_metrics_middleware_and_prometheus_export() {
    let registry = MetricsRegistry::new();
    let reg_for_mw = registry.clone();
    let reg_for_metrics = registry.clone();
    let reg_for_health = registry.clone();

    let app = Router::new()
        .route(
            "/healthz",
            get(move || {
                let r = reg_for_health.clone();
                async move {
                    (
                        StatusCode::OK,
                        Json(json!({ "status": "ok", "uptime_seconds": r.uptime_seconds() })),
                    )
                }
            }),
        )
        .route(
            "/api/grammar/{id}",
            get(|| async { (StatusCode::OK, Json(json!({ "id": 42 }))) }),
        )
        .route(
            "/metrics",
            get(move || {
                let r = reg_for_metrics.clone();
                async move {
                    (
                        StatusCode::OK,
                        [(header::CONTENT_TYPE, PROMETHEUS_CONTENT_TYPE)],
                        r.render_prometheus(),
                    )
                }
            }),
        )
        .layer(middleware::from_fn(move |req, next| {
            metrics_middleware(reg_for_mw.clone(), req, next)
        }));

    // 1. Hit /healthz
    let health_resp = app
        .clone()
        .oneshot(Request::builder().uri("/healthz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(health_resp.status(), StatusCode::OK);

    // 2. Hit dynamic route /api/grammar/42 to ensure route normalization collapses numeric IDs
    let grammar_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/grammar/42")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(grammar_resp.status(), StatusCode::OK);

    registry.inc_reviews_submitted();
    registry.inc_nlp_analyses();
    registry.inc_audio_requests();

    // 3. Scrape /metrics
    let metrics_resp = app
        .oneshot(Request::builder().uri("/metrics").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(metrics_resp.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(metrics_resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let text = String::from_utf8(body_bytes.to_vec()).unwrap();

    assert!(text.contains("http_requests_total"));
    assert!(text.contains("route=\"/api/grammar/:id\""));
    assert!(!text.contains("route=\"/api/grammar/42\""));
    assert!(text.contains("http_request_duration_seconds_bucket"));
    assert!(text.contains("bunamateur_reviews_submitted_total 1"));
    assert!(text.contains("bunamateur_nlp_analyses_total 1"));
    assert!(text.contains("bunamateur_audio_requests_total 1"));
    assert!(text.contains("http_requests_in_flight 1"));
    assert_eq!(registry.in_flight(), 0);
}

#[tokio::test]
async fn test_tower_oneshot_static_asset_cache_headers_and_304() {
    async fn serve_asset(uri: axum::http::Uri, headers: HeaderMap) -> impl IntoResponse {
        let path = uri.path().trim_start_matches('/');
        let payload = if path.starts_with("assets/") {
            b"console.log('bunamateur-bundle');".to_vec()
        } else {
            b"<!doctype html><title>BunAmateur</title>".to_vec()
        };
        let etag = compute_weak_etag(&payload);
        let cc = static_cache_control(path);
        if matches_if_none_match(&headers, &etag) {
            return (
                StatusCode::NOT_MODIFIED,
                [
                    (header::ETAG, HeaderValue::from_str(&etag).unwrap()),
                    (header::CACHE_CONTROL, HeaderValue::from_static(cc)),
                ],
                Vec::new(),
            );
        }
        (
            StatusCode::OK,
            [
                (header::ETAG, HeaderValue::from_str(&etag).unwrap()),
                (header::CACHE_CONTROL, HeaderValue::from_static(cc)),
            ],
            payload,
        )
    }

    let app = Router::new().fallback(serve_asset);

    // Hashed asset receives immutable 1-year Cache-Control + ETag
    let r1 = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/assets/index-DeadBeef.js")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r1.status(), StatusCode::OK);
    assert_eq!(
        r1.headers().get(header::CACHE_CONTROL).unwrap(),
        "public, max-age=31536000, immutable"
    );
    let etag = r1
        .headers()
        .get(header::ETAG)
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();

    // Conditional GET with matching If-None-Match yields 304 Not Modified
    let r2 = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/assets/index-DeadBeef.js")
                .header(header::IF_NONE_MATCH, etag)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r2.status(), StatusCode::NOT_MODIFIED);

    // index.html and sw.js are never immutably cached
    let r3 = app
        .clone()
        .oneshot(Request::builder().uri("/index.html").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(
        r3.headers().get(header::CACHE_CONTROL).unwrap(),
        "no-cache, no-store, must-revalidate"
    );

    let r4 = app
        .oneshot(Request::builder().uri("/sw.js").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(
        r4.headers().get(header::CACHE_CONTROL).unwrap(),
        "no-cache, no-store, must-revalidate"
    );

    // Gzip transport compression helper checks
    let mut hdrs = HeaderMap::new();
    hdrs.insert(header::ACCEPT_ENCODING, HeaderValue::from_static("br, gzip"));
    assert!(accepts_gzip(&hdrs));
    assert!(is_compressible_mime("application/javascript; charset=utf-8"));
    assert!(!is_compressible_mime("audio/mpeg"));
    let big_js = "export const bunamateur = '日本語文法SRS';\n".repeat(40);
    let gz = gzip_compress(big_js.as_bytes()).expect("compressible payload must gzip smaller");
    assert!(gz.len() < big_js.len());
    assert_eq!(&gz[0..2], &[0x1f, 0x8b]); // RFC 1952 gzip magic header
}

#[tokio::test]
async fn test_tower_oneshot_cors_rate_limit_and_body_limit() {
    let security = SecurityConfig {
        allowed_origins: vec!["https://app.bunamateur.test".to_string()],
        cookie_secure: false,
        session_ttl_days: 7,
        trust_proxy: false,
        rate_limits: RateLimitConfig {
            auth: RateLimitRule {
                max_requests: 2,
                window: Duration::from_secs(60),
            },
            wanikani: RateLimitRule {
                max_requests: 5,
                window: Duration::from_secs(60),
            },
            general: RateLimitRule {
                max_requests: 20,
                window: Duration::from_secs(60),
            },
        },
    };
    let limiter = RateLimiter::new(security.rate_limits.clone(), false);
    let cors = build_cors_layer(&security).unwrap();

    let app = Router::new()
        .route(
            "/api/auth/login",
            post(|_body: String| async { (StatusCode::OK, "ok") }),
        )
        .layer(axum::extract::DefaultBodyLimit::max(AUTH_BODY_LIMIT_BYTES))
        .layer(middleware::from_fn(move |req, next| {
            rate_limit_middleware(limiter.clone(), RateLimitTier::Auth, req, next)
        }))
        .layer(cors);

    // Allowed origin receives Access-Control-Allow-Origin
    let ok_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/auth/login")
                .header(header::ORIGIN, "https://app.bunamateur.test")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(ok_resp.status(), StatusCode::OK);
    assert_eq!(
        ok_resp
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .unwrap(),
        "https://app.bunamateur.test"
    );

    // Oversized body rejected with 413 Payload Too Large
    let big_body = "x".repeat(AUTH_BODY_LIMIT_BYTES + 128);
    let too_large = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/auth/login")
                .body(Body::from(big_body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(too_large.status(), StatusCode::PAYLOAD_TOO_LARGE);

    // Third request in same window hits 429 Too Many Requests
    let rate_limited = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/auth/login")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(rate_limited.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[test]
fn test_nlp_jmdict_wk_enrichment_and_lemmatized_grammar_matching() {
    let mut kanji_levels = HashMap::new();
    kanji_levels.insert("毎".to_string(), 5);
    kanji_levels.insert("日".to_string(), 2);
    kanji_levels.insert("読".to_string(), 10);
    let mut vocab_levels = HashMap::new();
    vocab_levels.insert("毎日".to_string(), 5);
    let mut known_kanji = HashSet::new();
    known_kanji.insert("毎".to_string());
    known_kanji.insert("日".to_string());
    known_kanji.insert("読".to_string());

    let wk = WkSubjectLookup {
        kanji_levels,
        vocab_levels,
        known_kanji,
    };

    let sample = "毎日日本語を勉強しているおかげで、少しずつ日本の新聞が読めるようになりました。";
    let analysis = analyze_japanese_text(sample, &wk);

    assert!(!analysis.tokens.is_empty());
    assert!(analysis.annotated_ruby.contains("[毎日|まいにち]"));
    assert!(analysis.annotated_ruby.contains("[読|よ]める"));

    let mainichi = analysis
        .tokens
        .iter()
        .find(|t| t.surface == "毎日")
        .expect("毎日 token should exist");
    assert_eq!(mainichi.wk_level, Some(5));
    assert!(mainichi.wk_known);
    assert!(!mainichi.glosses.is_empty());

    let yomeru = analysis
        .tokens
        .iter()
        .find(|t| t.surface == "読める")
        .expect("読める token should exist");
    assert_eq!(yomeru.lemma, "読む");
    assert_eq!(yomeru.wk_level, Some(10));

    // Lemmatized grammar pattern matching handles polite/past inflections
    assert!(match_pattern_against_analysis("ている", sample, &analysis).is_some());
    assert!(match_pattern_against_analysis("おかげで", sample, &analysis).is_some());
    assert!(match_pattern_against_analysis("ようになる", sample, &analysis).is_some());
}

#[tokio::test]
async fn test_audio_service_local_cache_and_fallback_flow() {
    let temp_dir = std::env::temp_dir().join(format!(
        "bunamateur_audio_suite_{}",
        Utc::now().timestamp_nanos_opt().unwrap_or(0)
    ));
    tokio::fs::create_dir_all(&temp_dir).await.unwrap();

    let text = "毎日日本語を勉強しています。";
    let hash = compute_audio_hash(text);
    let mp3_path = temp_dir.join(format!("{}.mp3", hash));
    tokio::fs::write(&mp3_path, b"ID3-fake-mp3-audio-payload")
        .await
        .unwrap();

    let svc = AudioService::new(AudioConfig {
        cache_dir: temp_dir.clone(),
        cdn_base_url: None,
        provider: AudioProviderKind::None,
        ..AudioConfig::default()
    });

    // Cached sentence resolves immediately as Bytes with audio/mpeg & ETag
    let hit = svc.resolve_sentence_audio(77, "", text).await.unwrap();
    match hit {
        AudioResolution::Bytes {
            bytes,
            mime_type,
            etag,
            source,
        } => {
            assert_eq!(bytes, b"ID3-fake-mp3-audio-payload");
            assert_eq!(mime_type, "audio/mpeg");
            assert!(etag.starts_with("W/\""));
            assert_eq!(source, "cache");
        }
        other => panic!("Expected cached Bytes, got {:?}", other),
    }

    // Uncached sentence returns NotAvailable (triggering client Web Speech fallback)
    let miss = svc
        .resolve_sentence_audio(78, "", "キャッシュにない文です。")
        .await
        .unwrap();
    assert!(matches!(miss, AudioResolution::NotAvailable));

    let _ = tokio::fs::remove_dir_all(&temp_dir).await;
}

#[test]
fn test_crypto_and_srs_nuance_grading_end_to_end() {
    let cipher = TokenCipher::from_key_str(
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
    )
    .unwrap();
    let enc = cipher.encrypt_for_user(101, "wk-secret-token").unwrap();
    assert_eq!(cipher.decrypt_for_user(101, &enc).unwrap(), "wk-secret-token");
    assert!(cipher.decrypt_for_user(102, &enc).is_err());

    // Register nuance produces non-penalizing AlmostRight shake hint
    let grade = grade_answer("ている", "ています", &[], "polite progressive");
    assert!(matches!(grade, GradeOutcome::AlmostRight { .. }));

    // Heavy ghost intensity spawns ghost level 4 on first miss
    let transition = compute_srs_transition_with_intensity(
        3,
        0,
        false,
        false,
        GhostIntensity::Heavy,
        Utc::now(),
    );
    assert_eq!(transition.new_ghost_level, 4);
}

#[test]
fn test_v02_password_reset_email_analytics_and_anki_tsv_export() {
    // 1. Password reset email escapes HTML and preserves reset URL
    let msg = build_password_reset_email(
        "learner@example.com",
        "Hana <N3>",
        "https://bunamateur.app/reset-password?token=abc123&src=mail",
    );
    assert_eq!(msg.to, "learner@example.com");
    assert!(msg.subject.to_lowercase().contains("reset"));
    assert!(msg.text.contains("https://bunamateur.app/reset-password?token=abc123&src=mail"));
    let html = msg.html.expect("HTML body should be populated");
    assert!(html.contains("Hana &lt;N3&gt;"));
    assert!(html.contains("token=abc123&amp;src=mail"));

    // 2. Longest streak and heatmap intensity bucketing
    use chrono::NaiveDate;
    let d = |y, m, day| NaiveDate::from_ymd_opt(y, m, day).unwrap();
    assert_eq!(compute_longest_streak(&[]), 0);
    assert_eq!(
        compute_longest_streak(&[
            d(2026, 1, 1),
            d(2026, 1, 2),
            d(2026, 1, 5),
            d(2026, 1, 6),
            d(2026, 1, 7),
            d(2026, 1, 8),
        ]),
        4
    );
    assert_eq!(review_intensity_bucket(0), 0);
    assert_eq!(review_intensity_bucket(3), 1);
    assert_eq!(review_intensity_bucket(10), 2);
    assert_eq!(review_intensity_bucket(20), 3);
    assert_eq!(review_intensity_bucket(45), 4);

    // 3. Anki TSV export sanitizes tabs & newlines cleanly
    let bookmarks = vec![VocabBookmark {
        id: 1,
        surface: "読める".to_string(),
        lemma: "読む".to_string(),
        reading: "よむ".to_string(),
        pos: "verb".to_string(),
        glosses: vec!["to read".to_string(), "to recite".to_string()],
        jlpt: Some("N5".to_string()),
        wk_level: Some(10),
        context_sentence: "新聞が\t読める\nようになりました。".to_string(),
        created_at: "2026-11-26T00:00:00Z".to_string(),
    }];
    let tsv = vocab_bookmarks_to_anki_tsv(&bookmarks);
    assert!(tsv.starts_with("#separator:tab\n"));
    let data_line = tsv.lines().last().unwrap();
    let cols: Vec<&str> = data_line.split('\t').collect();
    assert_eq!(cols.len(), 7);
    assert_eq!(cols[0], "読む");
    assert_eq!(cols[1], "よむ");
    assert_eq!(cols[2], "to read; to recite");
    assert_eq!(cols[3], "verb");
    assert_eq!(cols[4], "N5");
    assert_eq!(cols[5], "WK Lv.10");
    assert!(!cols[6].contains('\n'));
}

#[test]
fn test_email_verification_email_escapes_html_and_preserves_url() {
    let msg = build_email_verification_email(
        "sakura@example.com",
        "Sakura <N4> & \"Friends\"",
        "https://bunamateur.app/verify-email?token=verify_tok_123&ref=signup",
    );
    assert_eq!(msg.to, "sakura@example.com");
    assert!(msg.subject.to_lowercase().contains("verify"));
    assert!(
        msg.text
            .contains("https://bunamateur.app/verify-email?token=verify_tok_123&ref=signup")
    );
    assert!(msg.text.contains("24 hours"));
    let html = msg.html.expect("HTML body should be populated");
    assert!(html.contains("Sakura &lt;N4&gt; &amp; &quot;Friends&quot;"));
    assert!(html.contains("token=verify_tok_123&amp;ref=signup"));
    assert!(html.contains("24 hours"));
}

#[test]
fn test_lesson_loader_validator_and_api_json_shape() {
    validate_furigana("[食|た]べている", "sample").unwrap();
    assert!(validate_furigana("[食べる]", "bad").is_err());

    let lessons = load_embedded_lessons().expect("embedded lessons must load");
    assert!(lessons.contains_key("te-iru"));
    assert!(lessons.contains_key("koto-ni-suru"));
    assert_eq!(lessons.len(), 2);

    let known: HashSet<&str> = all_seed_grammar().iter().map(|g| g.slug).collect();
    for slug in lessons.keys() {
        assert!(
            known.contains(slug.as_str()),
            "lesson slug '{slug}' must exist in curriculum"
        );
    }

    let te = lessons.get("te-iru").unwrap();
    assert!(!te.mistakes.is_empty());
    assert!(!te.checks.is_empty());
    assert!(te.formation.rows.len() >= 5);

    // Round-trip through the public API DTOs so frontend contracts stay honest.
    let detail = GrammarPointDetail {
        id: 1,
        slug: "te-iru".into(),
        title: "〜ている".into(),
        reading: "ている".into(),
        meaning: "Ongoing".into(),
        jlpt_level: "N5".into(),
        study_path: "Genki I".into(),
        structure: "Vて + いる".into(),
        nuance: "short".into(),
        caution: "short caution".into(),
        confusable_with: "short contrast".into(),
        pitch_note: "".into(),
        user_notes: "".into(),
        srs_stage: 0,
        srs_label: "Lesson".into(),
        correct_count: 0,
        incorrect_count: 0,
        streak: 0,
        ghost_level: 0,
        next_review_at: None,
        sentences: vec![],
        wk_readiness_pct: 0,
        wk_known_kanji: 0,
        wk_total_kanji: 0,
        lesson: Some(te.clone()),
    };
    let detail_json = serde_json::to_value(&detail).unwrap();
    assert!(detail_json.get("lesson").is_some());
    assert_eq!(
        detail_json["lesson"]["checks"][0]["answer_idx"].as_u64(),
        Some(te.checks[0].answer_idx as u64)
    );

    let queue_item = ReviewQueueItem {
        grammar_id: 1,
        sentence_id: 10,
        title: detail.title.clone(),
        reading: detail.reading.clone(),
        meaning: detail.meaning.clone(),
        jlpt_level: detail.jlpt_level.clone(),
        structure: detail.structure.clone(),
        nuance: detail.nuance.clone(),
        caution: detail.caution.clone(),
        confusable_with: detail.confusable_with.clone(),
        jp_before: "[今|いま]、".into(),
        jp_after: "。".into(),
        answer: "ています".into(),
        alt_answers: vec![],
        hint: "polite".into(),
        english: "…".into(),
        nuance_note: "".into(),
        srs_stage: 0,
        srs_label: "Lesson".into(),
        is_ghost: false,
        ghost_level: 0,
        mode: "lesson".into(),
        lesson: Some(te.clone()),
        mistakes: te.mistakes.clone(),
    };
    let queue_json = serde_json::to_value(&queue_item).unwrap();
    assert!(queue_json.get("lesson").is_some());
    assert!(
        queue_json["mistakes"]
            .as_array()
            .map(|a| !a.is_empty())
            .unwrap_or(false)
    );

    // Reject malformed lesson JSON via the same parse path seed uses.
    let bad = r#"{"meaning":"x","gist":"","formation":{"columns":["a"],"rows":[["b"]],"note":""},"explanation":"e","senses":[],"register_context":"","mistakes":[],"contrasts":[],"examples":[],"dialogue":[],"checks":[],"related":{}}"#;
    assert!(parse_lesson_json("bad", bad).is_err());
}

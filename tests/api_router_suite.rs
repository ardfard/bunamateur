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
    nlp::{WkSubjectLookup, analyze_japanese_text, match_pattern_against_analysis},
    observability::{
        MetricsRegistry, PROMETHEUS_CONTENT_TYPE, accepts_gzip, compute_weak_etag, gzip_compress,
        is_compressible_mime, matches_if_none_match, metrics_middleware, static_cache_control,
    },
    security::{
        AUTH_BODY_LIMIT_BYTES, RateLimitConfig, RateLimitRule, RateLimitTier, RateLimiter,
        SecurityConfig, build_cors_layer, rate_limit_middleware,
    },
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

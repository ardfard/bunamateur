use axum::{
    Router,
    http::{HeaderMap, HeaderValue, StatusCode, Uri, header},
    middleware,
    response::{IntoResponse, Response},
};
use bunamateur::{
    audio::{AudioConfig, AudioService},
    crypto::TokenCipher,
    db::{
        AppState, DEFAULT_DATABASE_URL, sanitize_database_url, spawn_wanikani_auto_sync_worker,
    },
    email::{EmailConfig, build_email_sender},
    oauth::GoogleOAuthConfig,
    observability::{
        accepts_gzip, compute_weak_etag, gzip_compress, init_tracing, is_compressible_mime,
        matches_if_none_match, metrics_middleware, shutdown_signal, static_cache_control,
    },
    routes,
    security::{SecurityConfig, build_cors_layer},
};
use rust_embed::RustEmbed;
use std::net::SocketAddr;

#[derive(RustEmbed)]
#[folder = "frontend/dist"]
struct FrontendAssets;

fn build_asset_response(
    req_headers: &HeaderMap,
    path: &str,
    content_type: &str,
    bytes: Vec<u8>,
) -> Response {
    let etag = compute_weak_etag(&bytes);
    let cache_control = static_cache_control(path);

    if matches_if_none_match(req_headers, &etag) {
        let mut resp = StatusCode::NOT_MODIFIED.into_response();
        let headers = resp.headers_mut();
        if let Ok(val) = HeaderValue::from_str(&etag) {
            headers.insert(header::ETAG, val);
        }
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache_control));
        headers.insert(header::VARY, HeaderValue::from_static("Accept-Encoding"));
        return resp;
    }

    let (body, is_gzipped) =
        if bytes.len() >= 512 && is_compressible_mime(content_type) && accepts_gzip(req_headers) {
            match gzip_compress(&bytes) {
                Some(compressed) => (compressed, true),
                None => (bytes, false),
            }
        } else {
            (bytes, false)
        };

    let mut resp = (StatusCode::OK, body).into_response();
    let headers = resp.headers_mut();
    if let Ok(ct) = HeaderValue::from_str(content_type) {
        headers.insert(header::CONTENT_TYPE, ct);
    }
    if is_gzipped {
        headers.insert(header::CONTENT_ENCODING, HeaderValue::from_static("gzip"));
    }
    if let Ok(val) = HeaderValue::from_str(&etag) {
        headers.insert(header::ETAG, val);
    }
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache_control));
    headers.insert(header::VARY, HeaderValue::from_static("Accept-Encoding"));
    resp
}

async fn static_handler(uri: Uri, headers: HeaderMap) -> Response {
    let path = uri.path().trim_start_matches('/');

    // Try exact static asset first
    if !path.is_empty()
        && let Some(content) = FrontendAssets::get(path)
    {
        let mime = mime_guess::from_path(path).first_or_octet_stream();
        return build_asset_response(&headers, path, mime.as_ref(), content.data.into_owned());
    }

    // SPA fallback to index.html (never immutable-cached so new deploys take effect immediately)
    match FrontendAssets::get("index.html") {
        Some(content) => build_asset_response(
            &headers,
            "index.html",
            "text/html; charset=utf-8",
            content.data.into_owned(),
        ),
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            "Frontend assets not built yet. Run `cd frontend && bun run build` before `cargo build`.",
        )
            .into_response(),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let database_url =
        std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string());
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8080);
    let host = std::env::var("HOST").unwrap_or_else(|_| "127.0.0.1".to_string());

    let cipher = TokenCipher::from_env()?;
    let security = SecurityConfig::from_env(&host)?;
    let email_config = EmailConfig::from_env()?;
    let mailer = build_email_sender(&email_config)?;
    let google_oauth = GoogleOAuthConfig::from_env(&email_config.app_public_url);
    let audio_service = AudioService::new(AudioConfig::from_env());
    let cors = build_cors_layer(&security)?;

    let state = AppState::connect_with_config(&database_url, cipher, security)
        .await?
        .with_mailer(mailer, email_config.app_public_url.clone())
        .with_google_oauth(google_oauth)
        .with_audio(audio_service);
    tracing::info!(
        "PostgreSQL database connected, migrated, and seeded ({})",
        sanitize_database_url(&database_url)
    );
    tracing::info!(
        allowed_origins = ?state.security.allowed_origins,
        cookie_secure = state.security.cookie_secure,
        session_ttl_days = state.security.session_ttl_days,
        email_provider = state.mailer.provider_name(),
        google_oauth_enabled = state.google_oauth.is_enabled(),
        audio_provider = state.audio.config().provider_name(),
        "Security, auth & media configuration active"
    );

    spawn_wanikani_auto_sync_worker(state.clone());

    let api = routes::api_router(&state);
    let ops = routes::ops_router();
    let metrics = state.metrics.clone();
    let app = Router::new()
        .merge(ops)
        .nest("/api", api)
        .fallback(static_handler)
        .layer(middleware::from_fn(move |req, next| {
            metrics_middleware(metrics.clone(), req, next)
        }))
        .layer(cors)
        .with_state(state);

    let addr: SocketAddr = format!("{}:{}", host, port).parse()?;
    tracing::info!("BunAmateur (文アマ) running at http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;
    tracing::info!("BunAmateur (文アマ) shutdown complete");
    Ok(())
}

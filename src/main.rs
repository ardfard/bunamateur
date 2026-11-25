use axum::{
    Router,
    http::{StatusCode, Uri, header},
    response::{Html, IntoResponse, Response},
};
use bunamateur::{
    crypto::TokenCipher,
    db::{
        AppState, DEFAULT_DATABASE_URL, sanitize_database_url, spawn_wanikani_auto_sync_worker,
    },
    email::{EmailConfig, build_email_sender},
    oauth::GoogleOAuthConfig,
    routes,
    security::{SecurityConfig, build_cors_layer},
};
use rust_embed::RustEmbed;
use std::net::SocketAddr;
use tracing_subscriber::EnvFilter;

#[derive(RustEmbed)]
#[folder = "frontend/dist"]
struct FrontendAssets;

async fn static_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');

    // Try exact static asset first
    if !path.is_empty()
        && let Some(content) = FrontendAssets::get(path)
    {
        let mime = mime_guess::from_path(path).first_or_octet_stream();
        return (
            StatusCode::OK,
            [(header::CONTENT_TYPE, mime.as_ref().to_string())],
            content.data.into_owned(),
        )
            .into_response();
    }

    // SPA fallback to index.html
    match FrontendAssets::get("index.html") {
        Some(content) => Html(content.data.into_owned()).into_response(),
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            "Frontend assets not built yet. Run `cd frontend && bun run build` before `cargo build`.",
        )
            .into_response(),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "bunamateur=info,tower_http=info".into()),
        )
        .init();

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
    let cors = build_cors_layer(&security)?;

    let state = AppState::connect_with_config(&database_url, cipher, security)
        .await?
        .with_mailer(mailer, email_config.app_public_url.clone())
        .with_google_oauth(google_oauth);
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
        "Security & auth configuration active"
    );

    spawn_wanikani_auto_sync_worker(state.clone());

    let api = routes::api_router(&state);
    let app = Router::new()
        .nest("/api", api)
        .fallback(static_handler)
        .layer(cors)
        .with_state(state);

    let addr: SocketAddr = format!("{}:{}", host, port).parse()?;
    tracing::info!("BunAmateur (文アマ) running at http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}

mod db;
mod models;
mod routes;
mod seed_data;
mod srs;
mod wanikani;

use axum::{
    Router,
    http::{StatusCode, Uri, header},
    response::{Html, IntoResponse, Response},
};
use db::AppState;
use rust_embed::RustEmbed;
use std::net::SocketAddr;
use tower_http::cors::CorsLayer;
use tracing_subscriber::EnvFilter;

#[derive(RustEmbed)]
#[folder = "frontend/dist"]
struct FrontendAssets;

async fn static_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');

    // Try exact static asset first
    if !path.is_empty() {
        if let Some(content) = FrontendAssets::get(path) {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            return (
                StatusCode::OK,
                [(header::CONTENT_TYPE, mime.as_ref().to_string())],
                content.data.into_owned(),
            )
                .into_response();
        }
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
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "bunamateur=info,tower_http=info".into()))
        .init();

    let db_path = std::env::var("BUNAMATEUR_DB").unwrap_or_else(|_| "bunamateur.db".to_string());
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8080);
    let host = std::env::var("HOST").unwrap_or_else(|_| "127.0.0.1".to_string());

    let state = AppState::open(&db_path)?;
    tracing::info!("SQLite database initialized at {}", db_path);

    let app = Router::new()
        .nest("/api", routes::api_router())
        .fallback(static_handler)
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr: SocketAddr = format!("{}:{}", host, port).parse()?;
    tracing::info!("BunAmateur (文アマ) running at http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

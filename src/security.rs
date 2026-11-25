use anyhow::{Result, bail};
use axum::{
    Json,
    body::Body,
    extract::ConnectInfo,
    http::{HeaderMap, HeaderName, HeaderValue, Method, Request, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde_json::json;
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tower_http::cors::{AllowOrigin, CorsLayer};

pub const DEFAULT_BODY_LIMIT_BYTES: usize = 64 * 1024; // 64 KiB
pub const AUTH_BODY_LIMIT_BYTES: usize = 16 * 1024; // 16 KiB
pub const READING_ANALYZE_BODY_LIMIT_BYTES: usize = 256 * 1024; // 256 KiB

#[derive(Debug, Clone)]
pub struct RateLimitRule {
    pub max_requests: u32,
    pub window: Duration,
}

#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    pub auth: RateLimitRule,
    pub wanikani: RateLimitRule,
    pub general: RateLimitRule,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            auth: RateLimitRule {
                max_requests: 10,
                window: Duration::from_secs(60),
            },
            wanikani: RateLimitRule {
                max_requests: 15,
                window: Duration::from_secs(60),
            },
            general: RateLimitRule {
                max_requests: 180,
                window: Duration::from_secs(60),
            },
        }
    }
}

#[derive(Debug, Clone)]
pub struct SecurityConfig {
    pub allowed_origins: Vec<String>,
    pub cookie_secure: bool,
    pub session_ttl_days: i64,
    pub trust_proxy: bool,
    pub rate_limits: RateLimitConfig,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            allowed_origins: default_allowed_origins(),
            cookie_secure: false,
            session_ttl_days: 30,
            trust_proxy: false,
            rate_limits: RateLimitConfig::default(),
        }
    }
}

pub fn default_allowed_origins() -> Vec<String> {
    vec![
        "http://localhost:5173".to_string(),
        "http://127.0.0.1:5173".to_string(),
        "http://localhost:8080".to_string(),
        "http://127.0.0.1:8080".to_string(),
    ]
}

impl SecurityConfig {
    pub fn from_env(host: &str) -> Result<Self> {
        let allowed_origins = match std::env::var("ALLOWED_ORIGINS") {
            Ok(raw) if !raw.trim().is_empty() => {
                let origins: Vec<String> = raw
                    .split(',')
                    .map(|s| s.trim().trim_end_matches('/').to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                if origins.is_empty() {
                    bail!("ALLOWED_ORIGINS must contain at least one explicit origin");
                }
                if origins.iter().any(|o| o == "*") {
                    bail!(
                        "ALLOWED_ORIGINS cannot contain '*' when credentialed session cookies are enabled"
                    );
                }
                origins
            }
            _ => default_allowed_origins(),
        };

        let is_local_host = host == "127.0.0.1" || host == "localhost" || host == "::1";
        let is_production = std::env::var("APP_ENV")
            .or_else(|_| std::env::var("RUST_ENV"))
            .map(|v| v.eq_ignore_ascii_case("production"))
            .unwrap_or(false);

        let cookie_secure = match std::env::var("COOKIE_SECURE") {
            Ok(v) => matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"),
            Err(_) => is_production || !is_local_host,
        };

        let session_ttl_days = std::env::var("SESSION_TTL_DAYS")
            .ok()
            .and_then(|v| v.trim().parse::<i64>().ok())
            .map(|d| d.clamp(1, 365))
            .unwrap_or(30);

        let trust_proxy = std::env::var("TRUST_PROXY")
            .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
            .unwrap_or(false);

        Ok(Self {
            allowed_origins,
            cookie_secure,
            session_ttl_days,
            trust_proxy,
            rate_limits: RateLimitConfig::default(),
        })
    }
}

/// Builds a strict CORS layer restricted to configured `ALLOWED_ORIGINS` with credentials enabled.
pub fn build_cors_layer(config: &SecurityConfig) -> Result<CorsLayer> {
    let mut origins = Vec::with_capacity(config.allowed_origins.len());
    for origin in &config.allowed_origins {
        if origin == "*" {
            bail!("Wildcard '*' origin is not permitted with credentials");
        }
        let hv = HeaderValue::from_str(origin)
            .map_err(|e| anyhow::anyhow!("Invalid origin '{}' in ALLOWED_ORIGINS: {}", origin, e))?;
        origins.push(hv);
    }

    Ok(CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_credentials(true)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([
            header::CONTENT_TYPE,
            header::ACCEPT,
            HeaderName::from_static("x-csrf-token"),
        ])
        .max_age(Duration::from_secs(3600)))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RateLimitTier {
    Auth,
    WaniKani,
    General,
}

impl RateLimitTier {
    fn as_key(self) -> &'static str {
        match self {
            Self::Auth => "auth",
            Self::WaniKani => "wanikani",
            Self::General => "general",
        }
    }
}

#[derive(Debug, Clone)]
struct WindowEntry {
    count: u32,
    window_start: Instant,
}

#[derive(Debug, Clone)]
pub struct RateLimitDecision {
    pub allowed: bool,
    pub limit: u32,
    pub remaining: u32,
    pub retry_after_secs: u64,
}

/// In-memory fixed-window per-route rate limiter keyed by `(tier, client_ip)`.
#[derive(Clone)]
pub struct RateLimiter {
    config: RateLimitConfig,
    trust_proxy: bool,
    buckets: Arc<Mutex<HashMap<(String, String), WindowEntry>>>,
}

impl RateLimiter {
    pub fn new(config: RateLimitConfig, trust_proxy: bool) -> Self {
        Self {
            config,
            trust_proxy,
            buckets: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn rule_for(&self, tier: RateLimitTier) -> &RateLimitRule {
        match tier {
            RateLimitTier::Auth => &self.config.auth,
            RateLimitTier::WaniKani => &self.config.wanikani,
            RateLimitTier::General => &self.config.general,
        }
    }

    pub fn check(&self, tier: RateLimitTier, client_key: &str) -> RateLimitDecision {
        let rule = self.rule_for(tier);
        let now = Instant::now();
        let mut guard = self
            .buckets
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        // Opportunistic pruning to keep memory bounded under high cardinality
        if guard.len() > 10_000 {
            let max_window = self
                .config
                .auth
                .window
                .max(self.config.wanikani.window)
                .max(self.config.general.window);
            guard.retain(|_, entry| now.duration_since(entry.window_start) < max_window);
        }

        let key = (tier.as_key().to_string(), client_key.to_string());
        let entry = guard.entry(key).or_insert(WindowEntry {
            count: 0,
            window_start: now,
        });

        let elapsed = now.duration_since(entry.window_start);
        if elapsed >= rule.window {
            entry.count = 0;
            entry.window_start = now;
        }

        if entry.count >= rule.max_requests {
            let remaining_window = rule.window.saturating_sub(now.duration_since(entry.window_start));
            let retry_after_secs = remaining_window.as_secs().max(1);
            return RateLimitDecision {
                allowed: false,
                limit: rule.max_requests,
                remaining: 0,
                retry_after_secs,
            };
        }

        entry.count += 1;
        RateLimitDecision {
            allowed: true,
            limit: rule.max_requests,
            remaining: rule.max_requests.saturating_sub(entry.count),
            retry_after_secs: 0,
        }
    }

    pub fn extract_client_key(&self, headers: &HeaderMap, peer: Option<SocketAddr>) -> String {
        if self.trust_proxy {
            if let Some(xff) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok())
                && let Some(first) = xff.split(',').next().map(str::trim)
                && !first.is_empty()
            {
                return first.to_string();
            }
            if let Some(real_ip) = headers.get("x-real-ip").and_then(|v| v.to_str().ok()) {
                let trimmed = real_ip.trim();
                if !trimmed.is_empty() {
                    return trimmed.to_string();
                }
            }
        }

        peer.map(|addr| addr.ip().to_string())
            .unwrap_or_else(|| "local".to_string())
    }
}

/// Axum middleware that enforces per-route rate limits and attaches `X-RateLimit-*` headers.
pub async fn rate_limit_middleware(
    limiter: RateLimiter,
    tier: RateLimitTier,
    req: Request<Body>,
    next: Next,
) -> Response {
    let peer = req.extensions().get::<ConnectInfo<SocketAddr>>().map(|ci| ci.0);
    let client_key = limiter.extract_client_key(req.headers(), peer);
    let decision = limiter.check(tier, &client_key);

    if !decision.allowed {
        let mut resp = (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({
                "error": "Too many requests. Please slow down and try again shortly."
            })),
        )
            .into_response();
        let headers = resp.headers_mut();
        if let Ok(v) = HeaderValue::from_str(&decision.retry_after_secs.to_string()) {
            headers.insert(header::RETRY_AFTER, v);
        }
        if let Ok(v) = HeaderValue::from_str(&decision.limit.to_string()) {
            headers.insert(HeaderName::from_static("x-ratelimit-limit"), v);
        }
        headers.insert(
            HeaderName::from_static("x-ratelimit-remaining"),
            HeaderValue::from_static("0"),
        );
        return resp;
    }

    let mut resp = next.run(req).await;
    let headers = resp.headers_mut();
    if let Ok(v) = HeaderValue::from_str(&decision.limit.to_string()) {
        headers.insert(HeaderName::from_static("x-ratelimit-limit"), v);
    }
    if let Ok(v) = HeaderValue::from_str(&decision.remaining.to_string()) {
        headers.insert(HeaderName::from_static("x-ratelimit-remaining"), v);
    }
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limiter_blocks_after_limit_and_isolates_tiers() {
        let cfg = RateLimitConfig {
            auth: RateLimitRule {
                max_requests: 2,
                window: Duration::from_secs(60),
            },
            wanikani: RateLimitRule {
                max_requests: 3,
                window: Duration::from_secs(60),
            },
            general: RateLimitRule {
                max_requests: 5,
                window: Duration::from_secs(60),
            },
        };
        let limiter = RateLimiter::new(cfg, false);

        assert!(limiter.check(RateLimitTier::Auth, "1.2.3.4").allowed);
        assert!(limiter.check(RateLimitTier::Auth, "1.2.3.4").allowed);
        let blocked = limiter.check(RateLimitTier::Auth, "1.2.3.4");
        assert!(!blocked.allowed);
        assert!(blocked.retry_after_secs >= 1);

        // Different tier and different IP are unaffected
        assert!(limiter.check(RateLimitTier::General, "1.2.3.4").allowed);
        assert!(limiter.check(RateLimitTier::Auth, "5.6.7.8").allowed);
    }

    #[test]
    fn cors_rejects_wildcard_origin() {
        let cfg = SecurityConfig {
            allowed_origins: vec!["*".to_string()],
            ..SecurityConfig::default()
        };
        assert!(build_cors_layer(&cfg).is_err());
    }

    #[tokio::test]
    async fn middleware_enforces_cors_body_limit_and_rate_limit() {
        use axum::{Router, middleware, routing::post};
        use tower::ServiceExt;
        use tower_http::limit::RequestBodyLimitLayer;

        let cfg = SecurityConfig {
            allowed_origins: vec!["https://allowed.example".to_string()],
            rate_limits: RateLimitConfig {
                auth: RateLimitRule {
                    max_requests: 2,
                    window: Duration::from_secs(60),
                },
                ..RateLimitConfig::default()
            },
            ..SecurityConfig::default()
        };
        let cors = build_cors_layer(&cfg).unwrap();
        let limiter = RateLimiter::new(cfg.rate_limits.clone(), false);

        let app = Router::new()
            .route("/limited", post(|_body: String| async { StatusCode::OK }))
            .layer(axum::extract::DefaultBodyLimit::max(32))
            .layer(RequestBodyLimitLayer::new(32))
            .layer(middleware::from_fn(move |req, next| {
                rate_limit_middleware(limiter.clone(), RateLimitTier::Auth, req, next)
            }))
            .layer(cors);

        // Allowed CORS origin
        let ok_cors = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::OPTIONS)
                    .uri("/limited")
                    .header(header::ORIGIN, "https://allowed.example")
                    .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            ok_cors
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .and_then(|v| v.to_str().ok()),
            Some("https://allowed.example")
        );

        // Disallowed CORS origin
        let bad_cors = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::OPTIONS)
                    .uri("/limited")
                    .header(header::ORIGIN, "https://evil.example")
                    .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(
            bad_cors
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .is_none()
        );

        // Oversized body -> 413 Payload Too Large (counted as 1st request)
        let too_large = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/limited")
                    .body(Body::from("x".repeat(64)))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(too_large.status(), StatusCode::PAYLOAD_TOO_LARGE);

        // 2nd request succeeds
        let ok_req = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/limited")
                    .body(Body::from("ok"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(ok_req.status(), StatusCode::OK);

        // 3rd request hits Auth tier rate limit -> 429 Too Many Requests
        let rate_limited = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/limited")
                    .body(Body::from("ok"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(rate_limited.status(), StatusCode::TOO_MANY_REQUESTS);
        assert!(rate_limited.headers().get(header::RETRY_AFTER).is_some());
    }
}

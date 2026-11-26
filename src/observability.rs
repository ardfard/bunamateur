use axum::{
    extract::Request,
    http::{HeaderMap, header},
    middleware::Next,
    response::Response,
};
use chrono::{SecondsFormat, Utc};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt::{self, Write as _};
use std::sync::{
    Arc, RwLock,
    atomic::{AtomicI64, AtomicU64, Ordering},
};
use std::time::Instant;
use tracing::{
    Event, Subscriber,
    field::{Field, Visit},
};
use tracing_subscriber::{
    EnvFilter,
    fmt::{FmtContext, FormatEvent, FormatFields, format::Writer},
    registry::LookupSpan,
};

/// Default `RUST_LOG` filter when the environment variable is unset.
pub const DEFAULT_LOG_FILTER: &str = "bunamateur=info,tower_http=info";

/// Standard Prometheus text exposition format 0.0.4 media type.
pub const PROMETHEUS_CONTENT_TYPE: &str = "text/plain; version=0.0.4; charset=utf-8";

/// Histogram upper bounds (in seconds) for HTTP request latency tracking.
pub const HTTP_DURATION_BUCKETS: [f64; 10] = [
    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0,
];

/// Structured single-line JSON formatter for `tracing_subscriber`.
///
/// Emits JSON objects containing `timestamp`, `level`, `target`, `message`, and `fields`
/// without requiring the `tracing-subscriber/json` feature.
#[derive(Debug, Default, Clone, Copy)]
pub struct JsonEventFormatter;

struct JsonFieldVisitor {
    message: Option<String>,
    fields: Map<String, Value>,
}

impl JsonFieldVisitor {
    fn new() -> Self {
        Self {
            message: None,
            fields: Map::new(),
        }
    }

    fn record_value(&mut self, field: &Field, value: Value) {
        if field.name() == "message" {
            match value {
                Value::String(s) => self.message = Some(s),
                other => self.message = Some(other.to_string()),
            }
        } else {
            self.fields.insert(field.name().to_string(), value);
        }
    }
}

impl Visit for JsonFieldVisitor {
    fn record_f64(&mut self, field: &Field, value: f64) {
        let val = serde_json::Number::from_f64(value)
            .map(Value::Number)
            .unwrap_or_else(|| Value::String(value.to_string()));
        self.record_value(field, val);
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.record_value(field, Value::from(value));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.record_value(field, Value::from(value));
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.record_value(field, Value::from(value));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.record_value(field, Value::String(value.to_string()));
    }

    fn record_error(&mut self, field: &Field, value: &(dyn std::error::Error + 'static)) {
        self.record_value(field, Value::String(value.to_string()));
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.record_value(field, Value::String(format!("{value:?}")));
    }
}

impl<S, N> FormatEvent<S, N> for JsonEventFormatter
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        _ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let meta = event.metadata();
        let mut visitor = JsonFieldVisitor::new();
        event.record(&mut visitor);

        let payload = json!({
            "timestamp": Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            "level": meta.level().as_str(),
            "target": meta.target(),
            "message": visitor.message.unwrap_or_default(),
            "fields": Value::Object(visitor.fields),
        });

        let encoded = serde_json::to_string(&payload).map_err(|_| fmt::Error)?;
        writeln!(writer, "{encoded}")
    }
}

/// Initializes global structured logging using `RUST_LOG` and `LOG_FORMAT`.
///
/// - `RUST_LOG` defaults to `"bunamateur=info,tower_http=info"`.
/// - `LOG_FORMAT=json` enables single-line JSON output via [`JsonEventFormatter`].
/// - Any other value (or unset) uses human-readable `tracing_subscriber::fmt`.
pub fn init_tracing() {
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| DEFAULT_LOG_FILTER.into());
    let log_format = std::env::var("LOG_FORMAT").unwrap_or_default();

    if log_format.trim().eq_ignore_ascii_case("json") {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .event_format(JsonEventFormatter)
            .try_init();
    } else {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .try_init();
    }
}

#[derive(Debug, Default)]
struct DurationHistogram {
    bucket_counts: [u64; HTTP_DURATION_BUCKETS.len()],
    sum: f64,
    count: u64,
}

impl DurationHistogram {
    fn observe(&mut self, duration_secs: f64) {
        let d = if duration_secs.is_finite() && duration_secs >= 0.0 {
            duration_secs
        } else {
            0.0
        };
        for (idx, &bound) in HTTP_DURATION_BUCKETS.iter().enumerate() {
            if d <= bound {
                self.bucket_counts[idx] += 1;
            }
        }
        self.sum += d;
        self.count += 1;
    }
}

#[derive(Debug)]
struct MetricsInner {
    http_requests_in_flight: AtomicI64,
    http_requests_total: RwLock<BTreeMap<(String, String, u16), u64>>,
    http_request_duration_seconds: RwLock<DurationHistogram>,
    reviews_submitted_total: AtomicU64,
    auth_logins_total: AtomicU64,
    nlp_analyses_total: AtomicU64,
    audio_requests_total: AtomicU64,
    process_start_unix: AtomicU64,
}

/// Thread-safe in-process Prometheus metrics registry.
#[derive(Debug, Clone)]
pub struct MetricsRegistry {
    inner: Arc<MetricsInner>,
}

impl Default for MetricsRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricsRegistry {
    /// Creates a new registry initialized with the current Unix timestamp as start time.
    pub fn new() -> Self {
        let start_unix = u64::try_from(Utc::now().timestamp()).unwrap_or(0);
        Self {
            inner: Arc::new(MetricsInner {
                http_requests_in_flight: AtomicI64::new(0),
                http_requests_total: RwLock::new(BTreeMap::new()),
                http_request_duration_seconds: RwLock::new(DurationHistogram::default()),
                reviews_submitted_total: AtomicU64::new(0),
                auth_logins_total: AtomicU64::new(0),
                nlp_analyses_total: AtomicU64::new(0),
                audio_requests_total: AtomicU64::new(0),
                process_start_unix: AtomicU64::new(start_unix),
            }),
        }
    }

    /// Increments the total number of SRS reviews submitted.
    pub fn inc_reviews_submitted(&self) {
        self.inner
            .reviews_submitted_total
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Increments the total number of successful authentication logins.
    pub fn inc_auth_logins(&self) {
        self.inner.auth_logins_total.fetch_add(1, Ordering::Relaxed);
    }

    /// Increments the total number of Japanese NLP/morphology analyses performed.
    pub fn inc_nlp_analyses(&self) {
        self.inner
            .nlp_analyses_total
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Increments the total number of TTS/sentence audio requests handled.
    pub fn inc_audio_requests(&self) {
        self.inner
            .audio_requests_total
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Records a completed HTTP request's counter and latency histogram observation.
    pub fn record_http_request(&self, method: &str, route: &str, status: u16, duration_secs: f64) {
        let normalized_route = normalize_route_label(route);
        let key = (method.to_uppercase(), normalized_route, status);

        if let Ok(mut map) = self.inner.http_requests_total.write() {
            *map.entry(key).or_insert(0) += 1;
        }
        if let Ok(mut hist) = self.inner.http_request_duration_seconds.write() {
            hist.observe(duration_secs);
        }
    }

    /// Returns the current number of in-flight HTTP requests.
    pub fn in_flight(&self) -> i64 {
        self.inner.http_requests_in_flight.load(Ordering::Relaxed)
    }

    /// Returns the Unix timestamp (seconds) when this registry was created.
    pub fn process_start_unix(&self) -> u64 {
        self.inner.process_start_unix.load(Ordering::Relaxed)
    }

    /// Returns elapsed uptime in seconds since registry initialization.
    pub fn uptime_seconds(&self) -> u64 {
        let now = u64::try_from(Utc::now().timestamp()).unwrap_or(0);
        now.saturating_sub(self.process_start_unix())
    }

    /// Renders all tracked metrics in Prometheus text exposition format (version 0.0.4).
    pub fn render_prometheus(&self) -> String {
        let mut out = String::with_capacity(2048);

        let start_unix = self.process_start_unix();
        let uptime = self.uptime_seconds();
        let in_flight = self.in_flight();

        let _ = writeln!(
            out,
            "# HELP process_start_time_seconds Start time of the process since unix epoch in seconds."
        );
        let _ = writeln!(out, "# TYPE process_start_time_seconds gauge");
        let _ = writeln!(out, "process_start_time_seconds {start_unix}");

        let _ = writeln!(
            out,
            "# HELP bunamateur_uptime_seconds Elapsed process uptime in seconds."
        );
        let _ = writeln!(out, "# TYPE bunamateur_uptime_seconds gauge");
        let _ = writeln!(out, "bunamateur_uptime_seconds {uptime}");

        let _ = writeln!(
            out,
            "# HELP http_requests_in_flight Current number of HTTP requests being processed."
        );
        let _ = writeln!(out, "# TYPE http_requests_in_flight gauge");
        let _ = writeln!(out, "http_requests_in_flight {in_flight}");

        let _ = writeln!(
            out,
            "# HELP http_requests_total Total number of HTTP requests handled."
        );
        let _ = writeln!(out, "# TYPE http_requests_total counter");
        if let Ok(map) = self.inner.http_requests_total.read() {
            for ((method, route, status), count) in map.iter() {
                let escaped_method = escape_prometheus_label(method);
                let escaped_route = escape_prometheus_label(route);
                let _ = writeln!(
                    out,
                    "http_requests_total{{method=\"{escaped_method}\",route=\"{escaped_route}\",status=\"{status}\"}} {count}"
                );
            }
        }

        let _ = writeln!(
            out,
            "# HELP http_request_duration_seconds HTTP request latency distribution in seconds."
        );
        let _ = writeln!(out, "# TYPE http_request_duration_seconds histogram");
        if let Ok(hist) = self.inner.http_request_duration_seconds.read() {
            for (idx, &bound) in HTTP_DURATION_BUCKETS.iter().enumerate() {
                let count = hist.bucket_counts[idx];
                let _ = writeln!(
                    out,
                    "http_request_duration_seconds_bucket{{le=\"{bound}\"}} {count}"
                );
            }
            let _ = writeln!(
                out,
                "http_request_duration_seconds_bucket{{le=\"+Inf\"}} {}",
                hist.count
            );
            let _ = writeln!(out, "http_request_duration_seconds_sum {:.6}", hist.sum);
            let _ = writeln!(out, "http_request_duration_seconds_count {}", hist.count);
        }

        let reviews = self.inner.reviews_submitted_total.load(Ordering::Relaxed);
        let logins = self.inner.auth_logins_total.load(Ordering::Relaxed);
        let nlp = self.inner.nlp_analyses_total.load(Ordering::Relaxed);
        let audio = self.inner.audio_requests_total.load(Ordering::Relaxed);

        let _ = writeln!(
            out,
            "# HELP bunamateur_reviews_submitted_total Total number of SRS reviews submitted."
        );
        let _ = writeln!(out, "# TYPE bunamateur_reviews_submitted_total counter");
        let _ = writeln!(out, "bunamateur_reviews_submitted_total {reviews}");

        let _ = writeln!(
            out,
            "# HELP bunamateur_auth_logins_total Total number of successful user logins."
        );
        let _ = writeln!(out, "# TYPE bunamateur_auth_logins_total counter");
        let _ = writeln!(out, "bunamateur_auth_logins_total {logins}");

        let _ = writeln!(
            out,
            "# HELP bunamateur_nlp_analyses_total Total number of NLP morphology analyses performed."
        );
        let _ = writeln!(out, "# TYPE bunamateur_nlp_analyses_total counter");
        let _ = writeln!(out, "bunamateur_nlp_analyses_total {nlp}");

        let _ = writeln!(
            out,
            "# HELP bunamateur_audio_requests_total Total number of audio synthesis/playback requests."
        );
        let _ = writeln!(out, "# TYPE bunamateur_audio_requests_total counter");
        let _ = writeln!(out, "bunamateur_audio_requests_total {audio}");

        out
    }
}

struct InFlightGuard {
    inner: Arc<MetricsInner>,
}

impl InFlightGuard {
    fn new(metrics: &MetricsRegistry) -> Self {
        metrics
            .inner
            .http_requests_in_flight
            .fetch_add(1, Ordering::Relaxed);
        Self {
            inner: Arc::clone(&metrics.inner),
        }
    }
}

impl Drop for InFlightGuard {
    fn drop(&mut self) {
        self.inner
            .http_requests_in_flight
            .fetch_sub(1, Ordering::Relaxed);
    }
}

/// Axum middleware that tracks in-flight requests, request counts, and latency histograms.
///
/// Uses an RAII guard so `http_requests_in_flight` is decremented even if a downstream
/// handler panics or the request future is cancelled.
pub async fn metrics_middleware(
    metrics: MetricsRegistry,
    req: Request,
    next: Next,
) -> Response {
    let _in_flight = InFlightGuard::new(&metrics);
    let method = req.method().as_str().to_string();
    let route = normalize_route_label(req.uri().path());
    let start = Instant::now();

    let response = next.run(req).await;
    let status = response.status().as_u16();
    let duration_secs = start.elapsed().as_secs_f64();

    metrics.record_http_request(&method, &route, status, duration_secs);
    response
}

fn is_uuid_segment(segment: &str) -> bool {
    let bytes = segment.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    for (idx, &b) in bytes.iter().enumerate() {
        if matches!(idx, 8 | 13 | 18 | 23) {
            if b != b'-' {
                return false;
            }
        } else if !b.is_ascii_hexdigit() {
            return false;
        }
    }
    true
}

fn is_dynamic_id_segment(segment: &str) -> bool {
    if segment.is_empty() {
        return false;
    }
    if segment.bytes().all(|b| b.is_ascii_digit()) {
        return true;
    }
    if is_uuid_segment(segment) {
        return true;
    }
    segment.len() >= 16 && segment.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Normalizes HTTP request paths into low-cardinality Prometheus route labels.
///
/// Examples:
/// - `/api/grammar/12/notes` -> `/api/grammar/:id/notes`
/// - `/api/audio/sentence/5` -> `/api/audio/sentence/:id`
/// - `/assets/index-abc123.js` -> `/assets/:asset`
pub fn normalize_route_label(path: &str) -> String {
    let without_query = path.split(['?', '#']).next().unwrap_or(path);
    let trimmed = without_query.trim_end_matches('/');
    if trimmed.is_empty() || trimmed == "/" {
        return "/".to_string();
    }

    let leading = trimmed.trim_start_matches('/');
    if leading.starts_with("assets/") {
        return "/assets/:asset".to_string();
    }

    let mut normalized = String::with_capacity(trimmed.len());
    for segment in leading.split('/') {
        if segment.is_empty() {
            continue;
        }
        normalized.push('/');
        if is_dynamic_id_segment(segment) {
            normalized.push_str(":id");
        } else {
            normalized.push_str(segment);
        }
    }

    if normalized.is_empty() {
        "/".to_string()
    } else {
        normalized
    }
}

fn escape_prometheus_label(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

/// Returns an appropriate `Cache-Control` header value for an embedded static asset path.
pub fn static_cache_control(path: &str) -> &'static str {
    let clean = path.trim_start_matches('/');
    if clean.starts_with("assets/") {
        return "public, max-age=31536000, immutable";
    }
    if clean.is_empty() || clean == "index.html" || clean == "sw.js" {
        return "no-cache, no-store, must-revalidate";
    }
    if clean == "manifest.webmanifest" {
        return "public, max-age=86400";
    }
    if let Some(ext) = clean.rsplit('.').next()
        && clean.contains('.')
        && (ext.eq_ignore_ascii_case("svg")
            || ext.eq_ignore_ascii_case("png")
            || ext.eq_ignore_ascii_case("ico")
            || ext.eq_ignore_ascii_case("woff2"))
    {
        return "public, max-age=86400";
    }
    "public, max-age=3600"
}

/// Computes a deterministic weak ETag (`W/"<16-hex-chars>"`) from the SHA-256 digest of `bytes`.
pub fn compute_weak_etag(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let prefix = hex::encode(&digest[..8]);
    format!("W/\"{prefix}\"")
}

/// Evaluates whether any `If-None-Match` request header matches `etag` using RFC 9110 weak comparison.
pub fn matches_if_none_match(headers: &HeaderMap, etag: &str) -> bool {
    let target = strip_weak_prefix(etag.trim());
    for value in headers.get_all(header::IF_NONE_MATCH) {
        let Ok(raw) = value.to_str() else {
            continue;
        };
        for candidate in raw.split(',') {
            let trimmed = candidate.trim();
            if trimmed == "*" {
                return true;
            }
            if strip_weak_prefix(trimmed) == target {
                return true;
            }
        }
    }
    false
}

fn strip_weak_prefix(etag: &str) -> &str {
    etag.strip_prefix("W/").unwrap_or(etag)
}

/// Returns true if the request's `Accept-Encoding` header permits `gzip` responses.
pub fn accepts_gzip(headers: &HeaderMap) -> bool {
    for value in headers.get_all(header::ACCEPT_ENCODING) {
        let Ok(raw) = value.to_str() else {
            continue;
        };
        for item in raw.split(',') {
            let mut parts = item.trim().split(';');
            let enc = parts.next().unwrap_or("").trim();
            if enc.eq_ignore_ascii_case("gzip") {
                let explicitly_zero = parts.any(|p| {
                    let p = p.trim();
                    if let Some(q) = p.strip_prefix("q=") {
                        q.trim().parse::<f32>().map(|v| v <= 0.0).unwrap_or(false)
                    } else {
                        false
                    }
                });
                if !explicitly_zero {
                    return true;
                }
            }
        }
    }
    false
}

/// Returns true for MIME types that benefit from gzip transport compression.
pub fn is_compressible_mime(content_type: &str) -> bool {
    let base = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    base.starts_with("text/")
        || base == "application/javascript"
        || base == "application/json"
        || base == "application/manifest+json"
        || base == "image/svg+xml"
        || base == "application/wasm"
}

/// Compresses `bytes` with gzip, returning `Some(compressed)` only when compression reduces size.
pub fn gzip_compress(bytes: &[u8]) -> Option<Vec<u8>> {
    use flate2::{Compression, write::GzEncoder};
    use std::io::Write;

    let mut encoder = GzEncoder::new(Vec::with_capacity(bytes.len() / 2), Compression::fast());
    encoder.write_all(bytes).ok()?;
    let compressed = encoder.finish().ok()?;
    if compressed.len() < bytes.len() {
        Some(compressed)
    } else {
        None
    }
}

/// Waits for a graceful shutdown signal (`Ctrl+C` / `SIGINT`, or `SIGTERM` on Unix).
pub async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(err) = tokio::signal::ctrl_c().await {
            tracing::error!(error = %err, "Failed to install Ctrl+C signal handler");
            std::future::pending::<()>().await;
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut stream) => {
                stream.recv().await;
            }
            Err(err) => {
                tracing::error!(error = %err, "Failed to install SIGTERM signal handler");
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {
            tracing::info!("Received SIGINT (Ctrl+C); initiating graceful shutdown");
        }
        () = terminate => {
            tracing::info!("Received SIGTERM; initiating graceful shutdown");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Router,
        body::Body,
        http::{Request as HttpRequest, StatusCode},
        middleware,
        routing::get,
    };
    use std::io;
    use std::sync::Mutex;
    use tower::ServiceExt;
    use tracing_subscriber::fmt::MakeWriter;

    #[test]
    fn normalize_route_label_collapses_ids_and_tokens() {
        assert_eq!(normalize_route_label("/"), "/");
        assert_eq!(normalize_route_label(""), "/");
        assert_eq!(
            normalize_route_label("/api/grammar/12/notes"),
            "/api/grammar/:id/notes"
        );
        assert_eq!(
            normalize_route_label("/api/audio/sentence/5"),
            "/api/audio/sentence/:id"
        );
        assert_eq!(
            normalize_route_label("/api/grammar/42?include=examples"),
            "/api/grammar/:id"
        );
        assert_eq!(
            normalize_route_label("/api/users/550e8400-e29b-41d4-a716-446655440000/profile"),
            "/api/users/:id/profile"
        );
        assert_eq!(
            normalize_route_label("/api/auth/verify/0123456789abcdef0123456789abcdef"),
            "/api/auth/verify/:id"
        );
        assert_eq!(
            normalize_route_label("/assets/index-D9f2a1b3.js"),
            "/assets/:asset"
        );
        assert_eq!(normalize_route_label("/api/reviews/submit"), "/api/reviews/submit");
        assert_eq!(normalize_route_label("/healthz"), "/healthz");
    }

    #[test]
    fn render_prometheus_emits_valid_exposition_format() {
        let registry = MetricsRegistry::new();
        registry.inc_reviews_submitted();
        registry.inc_reviews_submitted();
        registry.inc_auth_logins();
        registry.inc_nlp_analyses();
        registry.inc_audio_requests();
        registry.record_http_request("get", "/api/grammar/12/notes", 200, 0.015);
        registry.record_http_request("POST", "/api/reviews/submit", 201, 0.300);

        let body = registry.render_prometheus();

        assert!(body.contains("# HELP process_start_time_seconds "));
        assert!(body.contains("# TYPE process_start_time_seconds gauge"));
        assert!(body.contains("# TYPE http_requests_in_flight gauge"));
        assert!(body.contains("http_requests_in_flight 0"));
        assert!(body.contains("# TYPE http_requests_total counter"));
        assert!(body.contains(
            "http_requests_total{method=\"GET\",route=\"/api/grammar/:id/notes\",status=\"200\"} 1"
        ));
        assert!(body.contains(
            "http_requests_total{method=\"POST\",route=\"/api/reviews/submit\",status=\"201\"} 1"
        ));
        assert!(body.contains("# TYPE http_request_duration_seconds histogram"));
        assert!(body.contains("http_request_duration_seconds_bucket{le=\"0.01\"} 0"));
        assert!(body.contains("http_request_duration_seconds_bucket{le=\"0.025\"} 1"));
        assert!(body.contains("http_request_duration_seconds_bucket{le=\"0.25\"} 1"));
        assert!(body.contains("http_request_duration_seconds_bucket{le=\"0.5\"} 2"));
        assert!(body.contains("http_request_duration_seconds_bucket{le=\"+Inf\"} 2"));
        assert!(body.contains("http_request_duration_seconds_sum 0.315000"));
        assert!(body.contains("http_request_duration_seconds_count 2"));
        assert!(body.contains("bunamateur_reviews_submitted_total 2"));
        assert!(body.contains("bunamateur_auth_logins_total 1"));
        assert!(body.contains("bunamateur_nlp_analyses_total 1"));
        assert!(body.contains("bunamateur_audio_requests_total 1"));
    }

    #[test]
    fn render_prometheus_escapes_label_characters() {
        let registry = MetricsRegistry::new();
        registry.record_http_request("GET", "/api/\"weird\"\\path\nnext", 400, 0.002);

        let body = registry.render_prometheus();
        assert!(body.contains("route=\"/api/\\\"weird\\\"\\\\path\\nnext\""));
    }

    #[test]
    fn static_cache_control_and_etag_helpers_work() {
        assert_eq!(
            static_cache_control("assets/app-abc123.js"),
            "public, max-age=31536000, immutable"
        );
        assert_eq!(
            static_cache_control("/assets/style-xyz.css"),
            "public, max-age=31536000, immutable"
        );
        assert_eq!(
            static_cache_control("index.html"),
            "no-cache, no-store, must-revalidate"
        );
        assert_eq!(
            static_cache_control("sw.js"),
            "no-cache, no-store, must-revalidate"
        );
        assert_eq!(
            static_cache_control(""),
            "no-cache, no-store, must-revalidate"
        );
        assert_eq!(
            static_cache_control("manifest.webmanifest"),
            "public, max-age=86400"
        );
        assert_eq!(static_cache_control("icons/logo.svg"), "public, max-age=86400");
        assert_eq!(static_cache_control("icons/icon-192.png"), "public, max-age=86400");
        assert_eq!(static_cache_control("favicon.ico"), "public, max-age=86400");
        assert_eq!(static_cache_control("fonts/noto.woff2"), "public, max-age=86400");
        assert_eq!(static_cache_control("robots.txt"), "public, max-age=3600");

        let etag = compute_weak_etag(b"hello bunamateur");
        assert!(etag.starts_with("W/\""));
        assert!(etag.ends_with('"'));
        assert_eq!(etag.len(), 20); // W/" + 16 hex chars + "
        assert_eq!(etag, compute_weak_etag(b"hello bunamateur"));
        assert_ne!(etag, compute_weak_etag(b"different content"));

        let mut headers = HeaderMap::new();
        assert!(!matches_if_none_match(&headers, &etag));

        headers.insert(header::IF_NONE_MATCH, etag.parse().unwrap());
        assert!(matches_if_none_match(&headers, &etag));

        // Strong equivalent should match under weak comparison
        let strong = etag.strip_prefix("W/").unwrap();
        headers.insert(header::IF_NONE_MATCH, strong.parse().unwrap());
        assert!(matches_if_none_match(&headers, &etag));

        // Comma-separated list
        let list = format!("W/\"0000000000000000\", {etag}");
        headers.insert(header::IF_NONE_MATCH, list.parse().unwrap());
        assert!(matches_if_none_match(&headers, &etag));

        // Wildcard
        headers.insert(header::IF_NONE_MATCH, "*".parse().unwrap());
        assert!(matches_if_none_match(&headers, &etag));

        // Non-matching tag
        headers.insert(header::IF_NONE_MATCH, "W/\"ffffffffffffffff\"".parse().unwrap());
        assert!(!matches_if_none_match(&headers, &etag));
    }

    #[derive(Clone, Default)]
    struct SharedBuf(Arc<Mutex<Vec<u8>>>);

    struct BufWriter(Arc<Mutex<Vec<u8>>>);

    impl io::Write for BufWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl<'a> MakeWriter<'a> for SharedBuf {
        type Writer = BufWriter;

        fn make_writer(&'a self) -> Self::Writer {
            BufWriter(Arc::clone(&self.0))
        }
    }

    #[test]
    fn json_event_formatter_outputs_structured_single_line_json() {
        let buf = SharedBuf::default();
        let subscriber = tracing_subscriber::fmt()
            .event_format(JsonEventFormatter)
            .with_writer(buf.clone())
            .finish();

        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(user_id = 42_u64, route = "/healthz", "request handled");
        });

        let raw = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
        let lines: Vec<&str> = raw.lines().collect();
        assert_eq!(lines.len(), 1);

        let parsed: Value = serde_json::from_str(lines[0]).expect("valid JSON log line");
        assert!(parsed.get("timestamp").and_then(Value::as_str).is_some());
        assert_eq!(parsed["level"], "INFO");
        assert!(parsed.get("target").and_then(Value::as_str).is_some());
        assert_eq!(parsed["message"], "request handled");
        assert_eq!(parsed["fields"]["user_id"], 42);
        assert_eq!(parsed["fields"]["route"], "/healthz");
    }

    #[tokio::test]
    async fn metrics_middleware_records_requests_and_clears_in_flight() {
        let metrics = MetricsRegistry::new();
        let metrics_for_layer = metrics.clone();

        let app = Router::new()
            .route("/api/grammar/{id}/notes", get(|| async { StatusCode::OK }))
            .route("/healthz", get(|| async { StatusCode::OK }))
            .layer(middleware::from_fn(move |req, next| {
                metrics_middleware(metrics_for_layer.clone(), req, next)
            }));

        let resp1 = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/grammar/12/notes")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp1.status(), StatusCode::OK);

        let resp2 = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/grammar/99/notes")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp2.status(), StatusCode::OK);

        let resp3 = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/missing/endpoint")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp3.status(), StatusCode::NOT_FOUND);

        assert_eq!(metrics.in_flight(), 0);

        let rendered = metrics.render_prometheus();
        assert!(rendered.contains(
            "http_requests_total{method=\"GET\",route=\"/api/grammar/:id/notes\",status=\"200\"} 2"
        ));
        assert!(rendered.contains(
            "http_requests_total{method=\"GET\",route=\"/missing/endpoint\",status=\"404\"} 1"
        ));
        assert!(rendered.contains("http_request_duration_seconds_count 3"));
    }

    #[test]
    fn in_flight_guard_decrements_on_drop() {
        let metrics = MetricsRegistry::new();
        assert_eq!(metrics.in_flight(), 0);
        {
            let _guard1 = InFlightGuard::new(&metrics);
            let _guard2 = InFlightGuard::new(&metrics);
            assert_eq!(metrics.in_flight(), 2);
        }
        assert_eq!(metrics.in_flight(), 0);
    }
}

use anyhow::{Context, Result, anyhow, bail};
use axum::http::HeaderValue;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::RngCore;
use reqwest::Client;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::time::Duration;
use subtle::ConstantTimeEq;

pub const GOOGLE_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
pub const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
pub const GOOGLE_USERINFO_URL: &str = "https://openidconnect.googleapis.com/v1/userinfo";
pub const OAUTH_STATE_COOKIE_NAME: &str = "bunamateur_oauth_state";
pub const OAUTH_STATE_TTL_SECS: i64 = 600; // 10 minutes

#[derive(Debug, Clone)]
pub struct GoogleOAuthConfig {
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub redirect_uri: String,
    pub auth_url: String,
    pub token_url: String,
    pub userinfo_url: String,
}

impl Default for GoogleOAuthConfig {
    fn default() -> Self {
        Self {
            client_id: None,
            client_secret: None,
            redirect_uri: "http://127.0.0.1:8080/api/auth/google/callback".to_string(),
            auth_url: GOOGLE_AUTH_URL.to_string(),
            token_url: GOOGLE_TOKEN_URL.to_string(),
            userinfo_url: GOOGLE_USERINFO_URL.to_string(),
        }
    }
}

impl GoogleOAuthConfig {
    pub fn from_env(app_public_url: &str) -> Self {
        let client_id = std::env::var("GOOGLE_CLIENT_ID")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());
        let client_secret = std::env::var("GOOGLE_CLIENT_SECRET")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());
        let redirect_uri = std::env::var("GOOGLE_REDIRECT_URI")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| {
                format!(
                    "{}/api/auth/google/callback",
                    app_public_url.trim_end_matches('/')
                )
            });

        Self {
            client_id,
            client_secret,
            redirect_uri,
            auth_url: std::env::var("GOOGLE_AUTH_URL")
                .ok()
                .filter(|v| !v.trim().is_empty())
                .unwrap_or_else(|| GOOGLE_AUTH_URL.to_string()),
            token_url: std::env::var("GOOGLE_TOKEN_URL")
                .ok()
                .filter(|v| !v.trim().is_empty())
                .unwrap_or_else(|| GOOGLE_TOKEN_URL.to_string()),
            userinfo_url: std::env::var("GOOGLE_USERINFO_URL")
                .ok()
                .filter(|v| !v.trim().is_empty())
                .unwrap_or_else(|| GOOGLE_USERINFO_URL.to_string()),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.client_id.is_some() && self.client_secret.is_some()
    }
}

#[derive(Debug, Clone)]
pub struct GoogleProfile {
    pub sub: String,
    pub email: String,
    pub email_verified: bool,
    pub name: Option<String>,
    pub picture: Option<String>,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
}

#[derive(Deserialize)]
struct UserInfoResponse {
    sub: String,
    email: String,
    #[serde(default)]
    email_verified: bool,
    name: Option<String>,
    picture: Option<String>,
}

/// Generates a high-entropy PKCE verifier (43 URL-safe base64 chars) and its S256 challenge.
pub fn generate_pkce_pair() -> (String, String) {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    let verifier = URL_SAFE_NO_PAD.encode(bytes);
    let digest = Sha256::digest(verifier.as_bytes());
    let challenge = URL_SAFE_NO_PAD.encode(digest);
    (verifier, challenge)
}

fn percent_encode_component(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for byte in raw.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => {
                out.push_str(&format!("%{:02X}", byte));
            }
        }
    }
    out
}

/// Builds the Google OAuth 2.0 Authorization Code URL with PKCE S256 & state.
pub fn build_google_authorize_url(
    cfg: &GoogleOAuthConfig,
    state: &str,
    code_challenge: &str,
) -> Result<String> {
    let client_id = cfg
        .client_id
        .as_deref()
        .ok_or_else(|| anyhow!("Google OAuth is not configured"))?;

    let params = [
        ("client_id", client_id),
        ("redirect_uri", cfg.redirect_uri.as_str()),
        ("response_type", "code"),
        ("scope", "openid email profile"),
        ("state", state),
        ("code_challenge", code_challenge),
        ("code_challenge_method", "S256"),
        ("prompt", "select_account"),
    ];

    let query: String = params
        .iter()
        .map(|(k, v)| format!("{}={}", k, percent_encode_component(v)))
        .collect::<Vec<_>>()
        .join("&");

    let sep = if cfg.auth_url.contains('?') { "&" } else { "?" };
    Ok(format!("{}{}{}", cfg.auth_url, sep, query))
}

/// Builds the short-lived HttpOnly cookie storing `<state>.<pkce_verifier>`.
pub fn build_oauth_state_cookie(state: &str, verifier: &str, secure: bool) -> Result<HeaderValue> {
    let secure_attr = if secure { "; Secure" } else { "" };
    let val = format!(
        "{}={}.{}; HttpOnly; SameSite=Lax; Path=/api/auth/google/callback; Max-Age={}{}",
        OAUTH_STATE_COOKIE_NAME, state, verifier, OAUTH_STATE_TTL_SECS, secure_attr
    );
    Ok(HeaderValue::from_str(&val)?)
}

/// Builds a cookie header that clears `bunamateur_oauth_state`.
pub fn build_clear_oauth_state_cookie(secure: bool) -> Result<HeaderValue> {
    let secure_attr = if secure { "; Secure" } else { "" };
    let val = format!(
        "{}=; HttpOnly; SameSite=Lax; Path=/api/auth/google/callback; Max-Age=0{}",
        OAUTH_STATE_COOKIE_NAME, secure_attr
    );
    Ok(HeaderValue::from_str(&val)?)
}

/// Validates the callback `state` against the stored cookie in constant time and returns `code_verifier`.
pub fn verify_oauth_state_cookie(cookie_value: &str, callback_state: &str) -> Option<String> {
    let (stored_state, verifier) = cookie_value.split_once('.')?;
    if stored_state.is_empty() || verifier.is_empty() || callback_state.is_empty() {
        return None;
    }
    let a = stored_state.as_bytes();
    let b = callback_state.as_bytes();
    if a.len() == b.len() && bool::from(a.ct_eq(b)) {
        Some(verifier.to_string())
    } else {
        None
    }
}

/// Exchanges the Google authorization code + PKCE verifier for an access token and fetches the OIDC user profile.
pub async fn exchange_code_for_google_profile(
    cfg: &GoogleOAuthConfig,
    code: &str,
    code_verifier: &str,
) -> Result<GoogleProfile> {
    let client_id = cfg
        .client_id
        .as_deref()
        .ok_or_else(|| anyhow!("Google OAuth client_id is missing"))?;
    let client_secret = cfg
        .client_secret
        .as_deref()
        .ok_or_else(|| anyhow!("Google OAuth client_secret is missing"))?;

    let client = Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .context("Failed to initialize HTTP client for Google OAuth")?;

    let token_resp = client
        .post(&cfg.token_url)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("redirect_uri", cfg.redirect_uri.as_str()),
            ("code_verifier", code_verifier),
        ])
        .send()
        .await
        .context("Failed to reach Google token endpoint")?;

    if !token_resp.status().is_success() {
        let status = token_resp.status().as_u16();
        let body = token_resp.text().await.unwrap_or_default();
        let snippet: String = body.chars().take(240).collect();
        bail!("Google token exchange failed (HTTP {}): {}", status, snippet);
    }

    let token_data: TokenResponse = token_resp
        .json()
        .await
        .context("Failed to parse Google token response")?;

    let userinfo_resp = client
        .get(&cfg.userinfo_url)
        .bearer_auth(&token_data.access_token)
        .send()
        .await
        .context("Failed to fetch Google userinfo")?;

    if !userinfo_resp.status().is_success() {
        let status = userinfo_resp.status().as_u16();
        bail!("Google userinfo request failed (HTTP {})", status);
    }

    let info: UserInfoResponse = userinfo_resp
        .json()
        .await
        .context("Failed to parse Google userinfo response")?;

    if info.sub.trim().is_empty() {
        bail!("Google userinfo missing subject identifier");
    }
    if !info.email_verified {
        bail!("Google account email is not verified");
    }

    Ok(GoogleProfile {
        sub: info.sub.trim().to_string(),
        email: info.email.trim().to_lowercase(),
        email_verified: info.email_verified,
        name: info.name.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
        picture: info
            .picture
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
    })
}

/// Derives a valid base username (2..=24 ASCII alnum/`_`/`-`) from a Google display name or email.
pub fn derive_username_base(name: Option<&str>, email: &str) -> String {
    let raw = name
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| email.split('@').next().unwrap_or("learner"));

    let mut cleaned: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c.to_ascii_lowercase()
            } else if c.is_whitespace() || c == '.' {
                '_'
            } else {
                '\0'
            }
        })
        .filter(|&c| c != '\0')
        .collect();

    while cleaned.contains("__") {
        cleaned = cleaned.replace("__", "_");
    }
    let trimmed = cleaned.trim_matches(|c| c == '_' || c == '-').to_string();
    let base: String = trimmed.chars().take(24).collect();
    if base.len() >= 2 {
        base
    } else {
        "learner".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Form, Json, Router, http::StatusCode, routing::{get, post}};
    use serde_json::json;
    use std::collections::HashMap;
    use tokio::net::TcpListener;

    #[test]
    fn pkce_and_authorize_url_generation() {
        let (verifier, challenge) = generate_pkce_pair();
        assert!(verifier.len() >= 43);
        assert!(challenge.len() >= 43);
        assert_ne!(verifier, challenge);

        let cfg = GoogleOAuthConfig {
            client_id: Some("cid-123".to_string()),
            client_secret: Some("csec-456".to_string()),
            redirect_uri: "https://app.example.com/api/auth/google/callback".to_string(),
            ..GoogleOAuthConfig::default()
        };
        let url = build_google_authorize_url(&cfg, "state_abc", &challenge).unwrap();
        assert!(url.starts_with(GOOGLE_AUTH_URL));
        assert!(url.contains("client_id=cid-123"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("state=state_abc"));

        assert_eq!(
            verify_oauth_state_cookie("state_abc.ver_xyz", "state_abc").as_deref(),
            Some("ver_xyz")
        );
        assert!(verify_oauth_state_cookie("state_abc.ver_xyz", "wrong_state").is_none());
    }

    #[test]
    fn username_derivation_sanitizes_input() {
        assert_eq!(derive_username_base(Some("Ardin Fardiansyah"), "a@b.com"), "ardin_fardiansyah");
        assert_eq!(derive_username_base(Some("さくら"), "sakura.jp@example.com"), "learner");
        assert_eq!(derive_username_base(None, "Kenji.Tanaka@example.com"), "kenji_tanaka");
    }

    #[tokio::test]
    async fn exchanges_code_and_rejects_unverified_email() {
        let mock_app = Router::new()
            .route(
                "/token",
                post(|Form(params): Form<HashMap<String, String>>| async move {
                    if params.get("code").map(String::as_str) == Some("good-code")
                        && params.get("code_verifier").map(String::as_str) == Some("good-verifier")
                    {
                        return (
                            StatusCode::OK,
                            Json(json!({ "access_token": "tok_verified" })),
                        );
                    }
                    if params.get("code").map(String::as_str) == Some("unverified-code") {
                        return (
                            StatusCode::OK,
                            Json(json!({ "access_token": "tok_unverified" })),
                        );
                    }
                    (StatusCode::BAD_REQUEST, Json(json!({ "error": "invalid_grant" })))
                }),
            )
            .route(
                "/userinfo",
                get(|headers: axum::http::HeaderMap| async move {
                    let auth = headers
                        .get(axum::http::header::AUTHORIZATION)
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("");
                    if auth == "Bearer tok_verified" {
                        return Json(json!({
                            "sub": "google-sub-42",
                            "email": "Yuki@Example.com",
                            "email_verified": true,
                            "name": "Yuki Sato",
                            "picture": "https://lh3.googleusercontent.com/avatar.png"
                        }));
                    }
                    Json(json!({
                        "sub": "google-sub-99",
                        "email": "unverified@example.com",
                        "email_verified": false,
                        "name": "NoVerify"
                    }))
                }),
            );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, mock_app).await.unwrap();
        });

        let cfg = GoogleOAuthConfig {
            client_id: Some("test-cid".to_string()),
            client_secret: Some("test-csec".to_string()),
            redirect_uri: "http://localhost:8080/api/auth/google/callback".to_string(),
            auth_url: format!("http://{}/auth", addr),
            token_url: format!("http://{}/token", addr),
            userinfo_url: format!("http://{}/userinfo", addr),
        };

        let profile = exchange_code_for_google_profile(&cfg, "good-code", "good-verifier")
            .await
            .unwrap();
        assert_eq!(profile.sub, "google-sub-42");
        assert_eq!(profile.email, "yuki@example.com");
        assert_eq!(profile.name.as_deref(), Some("Yuki Sato"));

        // Unverified email must be rejected
        assert!(
            exchange_code_for_google_profile(&cfg, "unverified-code", "good-verifier")
                .await
                .is_err()
        );

        server.abort();
    }
}

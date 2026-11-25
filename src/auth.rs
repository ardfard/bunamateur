use crate::db::{AppState, get_session_user};
use crate::models::AuthUserInfo;
use anyhow::{Result, anyhow, bail};
use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng},
};
use axum::{
    Json,
    extract::FromRequestParts,
    http::{HeaderMap, HeaderValue, StatusCode, header, request::Parts},
    response::{IntoResponse, Response},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::RngCore;
use serde_json::json;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

pub const SESSION_COOKIE_NAME: &str = "bunamateur_session";
pub const CSRF_COOKIE_NAME: &str = "bunamateur_csrf";
pub const CSRF_HEADER_NAME: &str = "x-csrf-token";

/// Dummy Argon2id hash used to equalize timing when logging in with an unknown email.
const DUMMY_ARGON2_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHRmb3J0aW1pbmc$1cG7Z3W8Y9mG6H1qP0tW0q4N3Z6mL9sP5kR8vX2nJ4Q";

/// Validates and normalizes an email address.
pub fn normalize_and_validate_email(raw: &str) -> Result<String> {
    let email = raw.trim().to_lowercase();
    if email.len() < 5 || email.len() > 254 {
        bail!("Please provide a valid email address");
    }
    let Some((local, domain)) = email.split_once('@') else {
        bail!("Please provide a valid email address");
    };
    if local.is_empty()
        || domain.is_empty()
        || !domain.contains('.')
        || domain.starts_with('.')
        || domain.ends_with('.')
        || email.chars().any(char::is_whitespace)
    {
        bail!("Please provide a valid email address");
    }
    Ok(email)
}

/// Validates and normalizes a username.
pub fn validate_username(raw: &str) -> Result<String> {
    let username = raw.trim();
    if username.len() < 2 || username.len() > 32 {
        bail!("Username must be between 2 and 32 characters");
    }
    if !username
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        bail!("Username may only contain letters, numbers, underscores, and hyphens");
    }
    Ok(username.to_string())
}

/// Validates password length requirements.
pub fn validate_password(password: &str) -> Result<()> {
    if password.len() < 8 {
        bail!("Password must be at least 8 characters long");
    }
    if password.len() > 128 {
        bail!("Password must be at most 128 characters long");
    }
    Ok(())
}

/// Hashes a password with Argon2id on a blocking thread pool.
pub async fn hash_password(password: String) -> Result<String> {
    tokio::task::spawn_blocking(move || {
        let salt = SaltString::generate(&mut OsRng);
        let argon2 = Argon2::default();
        argon2
            .hash_password(password.as_bytes(), &salt)
            .map(|h| h.to_string())
            .map_err(|e| anyhow!("Failed to hash password: {}", e))
    })
    .await
    .map_err(|e| anyhow!("Password hashing task failed: {}", e))?
}

/// Verifies a password against an Argon2id PHC string on a blocking thread pool.
pub async fn verify_password(password: String, hash: String) -> bool {
    tokio::task::spawn_blocking(move || {
        let parsed = match PasswordHash::new(&hash) {
            Ok(h) => h,
            Err(_) => {
                // Still spend Argon2 work against dummy hash to avoid obvious timing side-channel
                if let Ok(dummy) = PasswordHash::new(DUMMY_ARGON2_HASH) {
                    let _ = Argon2::default().verify_password(password.as_bytes(), &dummy);
                }
                return false;
            }
        };
        Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok()
    })
    .await
    .unwrap_or(false)
}

/// Performs a dummy Argon2id check when an account does not exist to mitigate timing enumeration.
pub async fn dummy_password_check(password: String) {
    let _ = verify_password(password, DUMMY_ARGON2_HASH.to_string()).await;
}

/// Generates a 256-bit cryptographically random URL-safe base64 token.
pub fn generate_random_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Hashes a raw session token using SHA-256 hex before storing or querying PostgreSQL.
pub fn hash_session_token(raw_token: &str) -> String {
    let digest = Sha256::digest(raw_token.as_bytes());
    hex::encode(digest)
}

/// Extracts a named cookie value from the `Cookie` request header(s).
pub fn extract_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    for value in headers.get_all(header::COOKIE) {
        let Ok(raw) = value.to_str() else {
            continue;
        };
        for pair in raw.split(';') {
            let trimmed = pair.trim();
            if let Some((k, v)) = trimmed.split_once('=')
                && k.trim() == name
            {
                let val = v.trim();
                if !val.is_empty() {
                    return Some(val.to_string());
                }
            }
        }
    }
    None
}

/// Builds the `Set-Cookie` headers for starting an authenticated session.
pub fn build_session_cookies(
    raw_session_token: &str,
    csrf_token: &str,
    max_age_secs: i64,
    secure: bool,
) -> Result<[HeaderValue; 2]> {
    let secure_attr = if secure { "; Secure" } else { "" };
    let session_cookie = format!(
        "{}={}; HttpOnly; SameSite=Lax; Path=/; Max-Age={}{}",
        SESSION_COOKIE_NAME, raw_session_token, max_age_secs, secure_attr
    );
    let csrf_cookie = format!(
        "{}={}; SameSite=Lax; Path=/; Max-Age={}{}",
        CSRF_COOKIE_NAME, csrf_token, max_age_secs, secure_attr
    );

    Ok([
        HeaderValue::from_str(&session_cookie)?,
        HeaderValue::from_str(&csrf_cookie)?,
    ])
}

/// Builds `Set-Cookie` headers that expire/clear both session and CSRF cookies.
pub fn build_clear_session_cookies(secure: bool) -> Result<[HeaderValue; 2]> {
    let secure_attr = if secure { "; Secure" } else { "" };
    let session_cookie = format!(
        "{}=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0{}",
        SESSION_COOKIE_NAME, secure_attr
    );
    let csrf_cookie = format!(
        "{}=; SameSite=Lax; Path=/; Max-Age=0{}",
        CSRF_COOKIE_NAME, secure_attr
    );

    Ok([
        HeaderValue::from_str(&session_cookie)?,
        HeaderValue::from_str(&csrf_cookie)?,
    ])
}

/// Authenticated user context extracted from the HTTP-only session cookie.
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user: AuthUserInfo,
    pub session_id_hash: String,
    pub csrf_token: String,
}

impl AuthUser {
    pub fn user_id(&self) -> i64 {
        self.user.id
    }
}

/// Rejection returned when authentication fails.
pub struct AuthRejection {
    status: StatusCode,
    message: &'static str,
}

impl IntoResponse for AuthRejection {
    fn into_response(self) -> Response {
        (self.status, Json(json!({ "error": self.message }))).into_response()
    }
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AuthRejection;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> std::result::Result<Self, Self::Rejection> {
        let Some(raw_token) = extract_cookie(&parts.headers, SESSION_COOKIE_NAME) else {
            return Err(AuthRejection {
                status: StatusCode::UNAUTHORIZED,
                message: "Authentication required",
            });
        };

        let session_id_hash = hash_session_token(&raw_token);
        match get_session_user(&state.pool, &session_id_hash).await {
            Ok(Some((user, csrf_token))) => Ok(AuthUser {
                user,
                session_id_hash,
                csrf_token,
            }),
            Ok(None) => Err(AuthRejection {
                status: StatusCode::UNAUTHORIZED,
                message: "Invalid or expired session",
            }),
            Err(e) => {
                tracing::error!("Session lookup failed: {}", e);
                Err(AuthRejection {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    message: "Authentication service error",
                })
            }
        }
    }
}

/// Constant-time verification of the `X-CSRF-Token` request header against the session's CSRF secret.
pub fn verify_csrf(headers: &HeaderMap, auth: &AuthUser) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    let provided = headers
        .get(CSRF_HEADER_NAME)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .unwrap_or("");

    let expected = auth.csrf_token.as_bytes();
    let given = provided.as_bytes();

    let valid = !given.is_empty() && given.len() == expected.len() && bool::from(given.ct_eq(expected));
    if !valid {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "Invalid or missing CSRF token" })),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn argon2id_hash_and_verify() {
        let hash = hash_password("correct-horse-battery".to_string())
            .await
            .unwrap();
        assert!(hash.starts_with("$argon2id$"));
        assert!(verify_password("correct-horse-battery".to_string(), hash.clone()).await);
        assert!(!verify_password("wrong-password".to_string(), hash).await);
    }

    #[test]
    fn cookie_parsing_and_formatting() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("theme=dark; bunamateur_session=tok123; bunamateur_csrf=csrf456"),
        );
        assert_eq!(
            extract_cookie(&headers, SESSION_COOKIE_NAME).as_deref(),
            Some("tok123")
        );
        assert_eq!(
            extract_cookie(&headers, CSRF_COOKIE_NAME).as_deref(),
            Some("csrf456")
        );

        let [sess, csrf] = build_session_cookies("tok123", "csrf456", 3600, true).unwrap();
        let sess_str = sess.to_str().unwrap();
        assert!(sess_str.contains("HttpOnly"));
        assert!(sess_str.contains("SameSite=Lax"));
        assert!(sess_str.contains("Secure"));
        assert!(csrf.to_str().unwrap().contains("Secure"));
        assert!(!csrf.to_str().unwrap().contains("HttpOnly"));
    }

    #[test]
    fn validation_helpers() {
        assert_eq!(
            normalize_and_validate_email("  Alice@Example.COM ").unwrap(),
            "alice@example.com"
        );
        assert!(normalize_and_validate_email("not-an-email").is_err());
        assert!(validate_username("akira_01").is_ok());
        assert!(validate_username("a").is_err());
        assert!(validate_username("bad name!").is_err());
        assert!(validate_password("1234567").is_err());
        assert!(validate_password("12345678").is_ok());
    }
}

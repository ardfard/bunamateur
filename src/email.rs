use anyhow::{Context, Result, anyhow, bail};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::{future::Future, pin::Pin, sync::Arc, time::Duration};

pub const DEFAULT_RESEND_API_BASE_URL: &str = "https://api.resend.com";
pub const DEFAULT_EMAIL_FROM: &str = "BunAmateur <onboarding@resend.dev>";
pub const DEFAULT_APP_PUBLIC_URL: &str = "http://127.0.0.1:8080";

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
pub type DynEmailSender = Arc<dyn EmailSender>;

/// Provider-agnostic transactional email payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmailMessage {
    pub to: String,
    pub subject: String,
    pub text: String,
    pub html: Option<String>,
    pub reply_to: Option<String>,
}

/// Metadata returned after a provider accepts a message for delivery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendEmailResult {
    pub provider: &'static str,
    pub message_id: Option<String>,
}

/// Pluggable email delivery interface so HTTP API providers (Resend) and future
/// SMTP transports (e.g., lettre) are interchangeable without touching callers.
pub trait EmailSender: Send + Sync {
    fn provider_name(&self) -> &'static str;

    fn send<'a>(&'a self, message: EmailMessage) -> BoxFuture<'a, Result<SendEmailResult>>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmailProviderKind {
    Resend,
    Smtp,
    Log,
}

#[derive(Debug, Clone)]
pub struct EmailConfig {
    pub provider: EmailProviderKind,
    pub from: String,
    pub app_public_url: String,
    pub resend_api_key: Option<String>,
    pub resend_api_base_url: String,
    // Reserved fields for future SMTP transport implementation behind `EmailSender`
    pub smtp_host: Option<String>,
    pub smtp_port: u16,
    pub smtp_username: Option<String>,
    pub smtp_password: Option<String>,
}

impl Default for EmailConfig {
    fn default() -> Self {
        Self {
            provider: EmailProviderKind::Log,
            from: DEFAULT_EMAIL_FROM.to_string(),
            app_public_url: DEFAULT_APP_PUBLIC_URL.to_string(),
            resend_api_key: None,
            resend_api_base_url: DEFAULT_RESEND_API_BASE_URL.to_string(),
            smtp_host: None,
            smtp_port: 587,
            smtp_username: None,
            smtp_password: None,
        }
    }
}

impl EmailConfig {
    /// Loads email provider configuration from environment variables.
    ///
    /// - `EMAIL_PROVIDER`: `"resend"` | `"smtp"` | `"log"`. When omitted, auto-selects
    ///   `"resend"` if `RESEND_API_KEY` is non-empty, otherwise `"log"`.
    /// - `EMAIL_FROM`: Sender address (default: `BunAmateur <onboarding@resend.dev>`).
    /// - `RESEND_API_KEY`: Resend API key for HTTP API delivery.
    /// - `RESEND_API_BASE_URL`: Optional Resend base URL override (default: `https://api.resend.com`).
    /// - `APP_PUBLIC_URL`: Public app URL included in transactional emails.
    /// - `SMTP_HOST`, `SMTP_PORT`, `SMTP_USERNAME`, `SMTP_PASSWORD`: Reserved for future SMTP transport.
    pub fn from_env() -> Result<Self> {
        let resend_api_key = std::env::var("RESEND_API_KEY")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());

        let provider = match std::env::var("EMAIL_PROVIDER") {
            Ok(raw) if !raw.trim().is_empty() => match raw.trim().to_ascii_lowercase().as_str() {
                "resend" | "resend_api" => EmailProviderKind::Resend,
                "smtp" => EmailProviderKind::Smtp,
                "log" | "noop" | "disabled" => EmailProviderKind::Log,
                other => {
                    bail!(
                        "Unsupported EMAIL_PROVIDER '{}'. Expected one of: resend, smtp, log",
                        other
                    );
                }
            },
            _ => {
                if resend_api_key.is_some() {
                    EmailProviderKind::Resend
                } else {
                    EmailProviderKind::Log
                }
            }
        };

        let from = std::env::var("EMAIL_FROM")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| DEFAULT_EMAIL_FROM.to_string());

        let app_public_url = std::env::var("APP_PUBLIC_URL")
            .ok()
            .map(|v| v.trim().trim_end_matches('/').to_string())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| DEFAULT_APP_PUBLIC_URL.to_string());

        let resend_api_base_url = std::env::var("RESEND_API_BASE_URL")
            .ok()
            .map(|v| v.trim().trim_end_matches('/').to_string())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| DEFAULT_RESEND_API_BASE_URL.to_string());

        let smtp_host = std::env::var("SMTP_HOST")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());
        let smtp_port = std::env::var("SMTP_PORT")
            .ok()
            .and_then(|v| v.trim().parse::<u16>().ok())
            .unwrap_or(587);
        let smtp_username = std::env::var("SMTP_USERNAME")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());
        let smtp_password = std::env::var("SMTP_PASSWORD")
            .ok()
            .filter(|v| !v.is_empty());

        Ok(Self {
            provider,
            from,
            app_public_url,
            resend_api_key,
            resend_api_base_url,
            smtp_host,
            smtp_port,
            smtp_username,
            smtp_password,
        })
    }
}

/// Instantiates the configured `EmailSender` behind a shared trait object.
pub fn build_email_sender(config: &EmailConfig) -> Result<DynEmailSender> {
    match config.provider {
        EmailProviderKind::Resend => {
            let api_key = config
                .resend_api_key
                .as_deref()
                .filter(|k| !k.trim().is_empty())
                .ok_or_else(|| {
                    anyhow!("RESEND_API_KEY is required when EMAIL_PROVIDER=resend")
                })?;
            let sender = ResendApiEmailSender::new(
                api_key.to_string(),
                config.from.clone(),
                config.resend_api_base_url.clone(),
            )?;
            Ok(Arc::new(sender))
        }
        EmailProviderKind::Smtp => {
            bail!(
                "EMAIL_PROVIDER=smtp is not enabled yet (SMTP transport can be added as an EmailSender impl). \
                 Use EMAIL_PROVIDER=resend (with RESEND_API_KEY) or EMAIL_PROVIDER=log."
            );
        }
        EmailProviderKind::Log => Ok(Arc::new(NoopLogEmailSender::new(config.from.clone()))),
    }
}

/// Resend HTTP API implementation of `EmailSender` (`POST /emails`).
#[derive(Clone)]
pub struct ResendApiEmailSender {
    client: Client,
    api_key: String,
    from: String,
    api_base_url: String,
}

#[derive(Serialize)]
struct ResendEmailRequest<'a> {
    from: &'a str,
    to: [&'a str; 1],
    subject: &'a str,
    text: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    html: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reply_to: Option<&'a str>,
}

#[derive(Deserialize)]
struct ResendEmailResponse {
    id: String,
}

impl ResendApiEmailSender {
    pub fn new(api_key: String, from: String, api_base_url: String) -> Result<Self> {
        if api_key.trim().is_empty() {
            bail!("Resend API key must not be empty");
        }
        if from.trim().is_empty() {
            bail!("EMAIL_FROM must not be empty");
        }
        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .context("Failed to construct HTTP client for Resend API")?;

        Ok(Self {
            client,
            api_key,
            from,
            api_base_url: api_base_url.trim_end_matches('/').to_string(),
        })
    }
}

impl EmailSender for ResendApiEmailSender {
    fn provider_name(&self) -> &'static str {
        "resend_api"
    }

    fn send<'a>(&'a self, message: EmailMessage) -> BoxFuture<'a, Result<SendEmailResult>> {
        Box::pin(async move {
            let url = format!("{}/emails", self.api_base_url);
            let payload = ResendEmailRequest {
                from: &self.from,
                to: [&message.to],
                subject: &message.subject,
                text: &message.text,
                html: message.html.as_deref(),
                reply_to: message.reply_to.as_deref(),
            };

            let resp = self
                .client
                .post(&url)
                .bearer_auth(&self.api_key)
                .json(&payload)
                .send()
                .await
                .with_context(|| format!("Failed to reach Resend API at {}", url))?;

            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            if !status.is_success() {
                let snippet: String = body.chars().take(300).collect();
                bail!("Resend API returned HTTP {}: {}", status.as_u16(), snippet);
            }

            let parsed: Option<ResendEmailResponse> = serde_json::from_str(&body).ok();
            Ok(SendEmailResult {
                provider: self.provider_name(),
                message_id: parsed.map(|p| p.id),
            })
        })
    }
}

/// Local development / fallback `EmailSender` that logs messages without external delivery.
#[derive(Clone)]
pub struct NoopLogEmailSender {
    from: String,
}

impl NoopLogEmailSender {
    pub fn new(from: String) -> Self {
        Self { from }
    }
}

impl EmailSender for NoopLogEmailSender {
    fn provider_name(&self) -> &'static str {
        "log"
    }

    fn send<'a>(&'a self, message: EmailMessage) -> BoxFuture<'a, Result<SendEmailResult>> {
        Box::pin(async move {
            tracing::info!(
                provider = "log",
                from = %self.from,
                to = %message.to,
                subject = %message.subject,
                "Email delivery skipped (EMAIL_PROVIDER=log; set RESEND_API_KEY to send via Resend)"
            );
            Ok(SendEmailResult {
                provider: self.provider_name(),
                message_id: None,
            })
        })
    }
}

fn escape_html(raw: &str) -> String {
    raw.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Builds a welcome email sent after account registration.
pub fn build_welcome_email(to: &str, username: &str, app_url: &str) -> EmailMessage {
    let safe_user = escape_html(username);
    let safe_url = escape_html(app_url);

    let text = format!(
        "Welcome to BunAmateur (文アマ), {username}!\n\n\
         Your Japanese grammar SRS account is ready. You can start your first JLPT lessons, \
         sync your WaniKani progress for Smart Furigana, and drill conjugations anytime:\n\
         {app_url}\n\n\
         Happy studying!\n\
         — BunAmateur"
    );

    let html = format!(
        "<div style=\"font-family: system-ui, -apple-system, sans-serif; max-width: 520px; margin: 0 auto; color: #1c1917;\">\
         <h2 style=\"color: #d94436;\">Welcome to BunAmateur (文アマ), {safe_user}!</h2>\
         <p>Your Japanese grammar SRS account is ready. Start your first JLPT lessons, connect WaniKani for Smart Furigana, and track Ghost Reviews anytime.</p>\
         <p style=\"margin: 24px 0;\"><a href=\"{safe_url}\" style=\"background: #d94436; color: #ffffff; padding: 10px 18px; border-radius: 8px; text-decoration: none; font-weight: 600;\">Open BunAmateur</a></p>\
         <p style=\"color: #78716c; font-size: 13px;\">Happy studying! — BunAmateur</p>\
         </div>"
    );

    EmailMessage {
        to: to.to_string(),
        subject: "Welcome to BunAmateur (文アマ)!".to_string(),
        text,
        html: Some(html),
        reply_to: None,
    }
}

/// Builds a password-reset email containing a single-use expiring link.
pub fn build_password_reset_email(to: &str, username: &str, reset_url: &str) -> EmailMessage {
    let safe_user = escape_html(username);
    let safe_url = escape_html(reset_url);

    let text = format!(
        "Hi {username},\n\n\
         We received a request to reset your BunAmateur (文アマ) password.\n\
         Open the link below within 60 minutes to choose a new password:\n\
         {reset_url}\n\n\
         If you didn't request this, you can safely ignore this email — your password will stay unchanged.\n\
         — BunAmateur"
    );

    let html = format!(
        "<div style=\"font-family: system-ui, -apple-system, sans-serif; max-width: 520px; margin: 0 auto; color: #1c1917;\">\
         <h2 style=\"color: #d94436;\">Reset your BunAmateur password</h2>\
         <p>Hi {safe_user}, we received a request to reset your password. Click below within 60 minutes to set a new one:</p>\
         <p style=\"margin: 24px 0;\"><a href=\"{safe_url}\" style=\"background: #d94436; color: #ffffff; padding: 10px 18px; border-radius: 8px; text-decoration: none; font-weight: 600;\">Reset Password</a></p>\
         <p style=\"color: #78716c; font-size: 13px;\">If you did not request a password reset, you can safely ignore this email.</p>\
         </div>"
    );

    EmailMessage {
        to: to.to_string(),
        subject: "Reset your BunAmateur (文アマ) password".to_string(),
        text,
        html: Some(html),
        reply_to: None,
    }
}

/// Builds a welcome + email verification message containing a single-use 24-hour link.
pub fn build_email_verification_email(to: &str, username: &str, verify_url: &str) -> EmailMessage {
    let safe_user = escape_html(username);
    let safe_url = escape_html(verify_url);

    let text = format!(
        "Welcome to BunAmateur (文アマ), {username}!\n\n\
         Your Japanese grammar SRS account is ready. Please confirm your email address \
         by opening the link below within 24 hours:\n\
         {verify_url}\n\n\
         If you didn't create a BunAmateur account, you can safely ignore this email.\n\
         — BunAmateur"
    );

    let html = format!(
        "<div style=\"font-family: system-ui, -apple-system, sans-serif; max-width: 520px; margin: 0 auto; color: #1c1917;\">\
         <h2 style=\"color: #d94436;\">Welcome to BunAmateur (文アマ), {safe_user}!</h2>\
         <p>Your Japanese grammar SRS account is ready. Please verify your email address within 24 hours so we can keep your account secure and send password recovery emails if needed:</p>\
         <p style=\"margin: 24px 0;\"><a href=\"{safe_url}\" style=\"background: #d94436; color: #ffffff; padding: 10px 18px; border-radius: 8px; text-decoration: none; font-weight: 600;\">Verify Email Address</a></p>\
         <p style=\"color: #78716c; font-size: 13px;\">This link expires in 24 hours. If you did not create a BunAmateur account, you can safely ignore this email.</p>\
         </div>"
    );

    EmailMessage {
        to: to.to_string(),
        subject: "Verify your BunAmateur (文アマ) email address".to_string(),
        text,
        html: Some(html),
        reply_to: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Json, Router,
        http::{HeaderMap, StatusCode, header},
        routing::post,
    };
    use serde_json::{Value, json};
    use std::sync::Mutex;
    use tokio::net::TcpListener;

    #[test]
    fn build_email_sender_validates_provider_configs() {
        let log_sender = build_email_sender(&EmailConfig::default()).unwrap();
        assert_eq!(log_sender.provider_name(), "log");

        let missing_key = EmailConfig {
            provider: EmailProviderKind::Resend,
            resend_api_key: None,
            ..EmailConfig::default()
        };
        assert!(build_email_sender(&missing_key).is_err());

        let smtp_cfg = EmailConfig {
            provider: EmailProviderKind::Smtp,
            ..EmailConfig::default()
        };
        assert!(build_email_sender(&smtp_cfg).is_err());
    }

    #[tokio::test]
    async fn noop_sender_works_via_trait_object() {
        let sender: DynEmailSender = Arc::new(NoopLogEmailSender::new(
            "BunAmateur <onboarding@resend.dev>".to_string(),
        ));
        let msg = build_welcome_email("alice@example.com", "alice_jp", "https://example.com");
        let res = sender.send(msg).await.unwrap();
        assert_eq!(res.provider, "log");
        assert_eq!(res.message_id, None);
    }

    #[tokio::test]
    async fn resend_api_sender_posts_expected_payload_and_handles_errors() {
        let captured: Arc<Mutex<Option<(String, Value)>>> = Arc::new(Mutex::new(None));
        let captured_ok = captured.clone();

        let mock_app = Router::new().route(
            "/emails",
            post(move |headers: HeaderMap, Json(body): Json<Value>| {
                let captured = captured_ok.clone();
                async move {
                    let auth = headers
                        .get(header::AUTHORIZATION)
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("")
                        .to_string();

                    if auth == "Bearer re_bad_key" {
                        return (
                            StatusCode::UNAUTHORIZED,
                            Json(json!({ "message": "Invalid API key" })),
                        );
                    }

                    *captured.lock().unwrap() = Some((auth, body));
                    (StatusCode::OK, Json(json!({ "id": "re_msg_012345" })))
                }
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, mock_app).await.unwrap();
        });

        let cfg = EmailConfig {
            provider: EmailProviderKind::Resend,
            from: "BunAmateur <hello@bunamateur.test>".to_string(),
            resend_api_key: Some("re_test_secret_123".to_string()),
            resend_api_base_url: format!("http://{}", addr),
            ..EmailConfig::default()
        };
        let sender: DynEmailSender = build_email_sender(&cfg).unwrap();
        assert_eq!(sender.provider_name(), "resend_api");

        let mut msg = build_welcome_email(
            "learner@example.com",
            "sakura<jp>",
            "https://macbook-pro.tail2a8683.ts.net",
        );
        msg.reply_to = Some("support@bunamateur.test".to_string());

        let result = sender.send(msg).await.unwrap();
        assert_eq!(result.provider, "resend_api");
        assert_eq!(result.message_id.as_deref(), Some("re_msg_012345"));

        let (auth_header, payload) = captured.lock().unwrap().take().unwrap();
        assert_eq!(auth_header, "Bearer re_test_secret_123");
        assert_eq!(payload["from"], "BunAmateur <hello@bunamateur.test>");
        assert_eq!(payload["to"], json!(["learner@example.com"]));
        assert_eq!(payload["reply_to"], "support@bunamateur.test");
        assert!(payload["html"].as_str().unwrap().contains("sakura&lt;jp&gt;"));

        // Verify non-2xx Resend API error surfaces cleanly
        let bad_sender = ResendApiEmailSender::new(
            "re_bad_key".to_string(),
            "BunAmateur <hello@bunamateur.test>".to_string(),
            format!("http://{}", addr),
        )
        .unwrap();
        let err = bad_sender
            .send(EmailMessage {
                to: "learner@example.com".to_string(),
                subject: "Test".to_string(),
                text: "Hello".to_string(),
                html: None,
                reply_to: None,
            })
            .await
            .unwrap_err();
        assert!(err.to_string().contains("401"));

        server.abort();
    }
}

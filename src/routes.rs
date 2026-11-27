use crate::audio::AudioResolution;
use crate::auth::{
    AuthUser, build_clear_session_cookies, build_session_cookies, dummy_password_check,
    extract_cookie, generate_random_token, hash_password, hash_session_token,
    normalize_and_validate_email, validate_password, validate_username, verify_csrf,
    verify_password,
};
use crate::db::{
    AppState, CreateUserError, analyze_reading_text, build_review_queue,
    complete_onboarding as db_complete_onboarding, compute_dashboard, compute_study_analytics,
    consume_email_verification_token, consume_password_reset_token,
    create_email_verification_token, create_password_reset_token, create_session, create_user,
    delete_session, delete_vocab_bookmark as db_delete_vocab_bookmark,
    disconnect_wanikani as db_disconnect_wanikani, export_user_progress, find_user_by_email,
    get_account_profile as db_get_account_profile, get_content_version, get_grammar_detail,
    get_placement_questions, get_sentence_answers, get_sentence_audio_info, get_user_password_hash,
    get_user_progress_stages, get_user_srs_settings, get_wanikani_credentials,
    import_user_progress, list_conjugations, list_grammar_summaries, list_reading_passages,
    list_user_sessions, list_vocab_bookmarks, load_wanikani_status, load_wanikani_sync_status,
    ping_database, progress_export_to_csv, record_review_submission,
    reset_grammar_progress as db_reset_grammar_progress,
    reset_onboarding as db_reset_onboarding, revoke_other_user_sessions,
    revoke_user_session, save_wanikani_token_and_start_sync,
    spawn_wanikani_sync_job, start_lesson as db_start_lesson, undo_review as db_undo_review,
    update_grammar_notes as db_update_grammar_notes, update_srs_settings as db_update_srs_settings,
    update_user_password_hash, update_user_username,
    update_wanikani_settings as db_update_wanikani_settings, upsert_google_user,
    upsert_vocab_bookmark, vocab_bookmarks_to_anki_tsv,
};
use crate::email::{
    build_email_verification_email, build_password_reset_email, build_welcome_email,
};
use crate::models::{
    AuthProvidersResponse, AuthResponse, ChangePasswordRequest, CompleteOnboardingRequest,
    ForgotPasswordRequest, ImportProgressRequest, LoginRequest, ReadingAnalyzeRequest,
    RegisterRequest, ResetPasswordRequest, SaveVocabBookmarkRequest, StartLessonRequest,
    SubmitAnswerRequest, SubmitAnswerResponse, UndoReviewRequest, UpdateNotesRequest,
    UpdateProfileRequest, UpdateSrsSettingsRequest, VerifyEmailRequest, WaniKaniConnectRequest,
    WaniKaniSettingsRequest,
};
use crate::oauth::{
    OAUTH_STATE_COOKIE_NAME, build_clear_oauth_state_cookie, build_google_authorize_url,
    build_oauth_state_cookie, exchange_code_for_google_profile, generate_pkce_pair,
    verify_oauth_state_cookie,
};
use crate::observability::{PROMETHEUS_CONTENT_TYPE, matches_if_none_match};
use crate::security::{
    AUTH_BODY_LIMIT_BYTES, DEFAULT_BODY_LIMIT_BYTES, READING_ANALYZE_BODY_LIMIT_BYTES,
    RateLimitTier, rate_limit_middleware,
};
use crate::srs::{
    GhostIntensity, GradeOutcome, compute_srs_transition_with_intensity, grade_answer,
    srs_interval_hours, srs_stage_label,
};
use crate::wanikani::WaniKaniClient;
use axum::{
    Json, Router,
    extract::{ConnectInfo, DefaultBodyLimit, Path, Query, State},
    http::{Extensions, HeaderMap, HeaderValue, StatusCode, header},
    middleware,
    response::{IntoResponse, Redirect, Response},
    routing::{delete, get, post, put},
};
use chrono::{Duration, Utc};
use serde::Deserialize;
use serde_json::json;
use std::net::SocketAddr;
use subtle::ConstantTimeEq;
use tower_http::limit::RequestBodyLimitLayer;

/// Public operational endpoints mounted at the application root:
/// `/healthz`, `/readyz`, and `/metrics`.
pub fn ops_router() -> Router<AppState> {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/metrics", get(metrics_handler))
}

async fn healthz(State(state): State<AppState>) -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(json!({
            "status": "ok",
            "service": "bunamateur",
            "uptime_seconds": state.metrics.uptime_seconds(),
        })),
    )
}

async fn readyz(State(state): State<AppState>) -> impl IntoResponse {
    match ping_database(&state.pool).await {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({
                "status": "ready",
                "database": "ok",
                "audio_provider": state.audio.config().provider_name(),
            })),
        ),
        Err(e) => {
            tracing::error!(error = %e, "Readiness check failed");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({
                    "status": "not_ready",
                    "database": "error",
                })),
            )
        }
    }
}

async fn metrics_handler(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Ok(expected) = std::env::var("METRICS_BEARER_TOKEN") {
        let trimmed = expected.trim();
        if !trimmed.is_empty() {
            let supplied = headers
                .get(header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.strip_prefix("Bearer "))
                .unwrap_or("");
            if supplied.as_bytes().ct_eq(trimmed.as_bytes()).unwrap_u8() == 0 {
                return (
                    StatusCode::UNAUTHORIZED,
                    [(header::WWW_AUTHENTICATE, "Bearer")],
                    "Unauthorized",
                )
                    .into_response();
            }
        }
    }

    let body = state.metrics.render_prometheus();
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, PROMETHEUS_CONTENT_TYPE)],
        body,
    )
        .into_response()
}

pub fn api_router(state: &AppState) -> Router<AppState> {
    let auth_limiter = state.rate_limiter.clone();
    let auth_public_routes = Router::new()
        .route("/auth/register", post(register))
        .route("/auth/login", post(login))
        .route("/auth/verify-email", post(verify_email))
        .route("/auth/resend-verification", post(resend_verification))
        .route("/auth/forgot-password", post(forgot_password))
        .route("/auth/reset-password", post(reset_password))
        .route("/auth/google/start", get(google_oauth_start))
        .route("/auth/google/callback", get(google_oauth_callback))
        .layer(DefaultBodyLimit::max(AUTH_BODY_LIMIT_BYTES))
        .layer(RequestBodyLimitLayer::new(AUTH_BODY_LIMIT_BYTES))
        .layer(middleware::from_fn(move |req, next| {
            rate_limit_middleware(auth_limiter.clone(), RateLimitTier::Auth, req, next)
        }));

    let wk_limiter = state.rate_limiter.clone();
    let wk_sync_routes = Router::new()
        .route("/wanikani/connect", post(connect_wanikani))
        .route("/wanikani/sync", post(resync_wanikani))
        .layer(DefaultBodyLimit::max(DEFAULT_BODY_LIMIT_BYTES))
        .layer(RequestBodyLimitLayer::new(DEFAULT_BODY_LIMIT_BYTES))
        .layer(middleware::from_fn(move |req, next| {
            rate_limit_middleware(wk_limiter.clone(), RateLimitTier::WaniKani, req, next)
        }));

    let reading_limiter = state.rate_limiter.clone();
    let large_body_routes = Router::new()
        .route("/reading/analyze", post(analyze_reading))
        .route("/import/progress", post(import_progress))
        .layer(DefaultBodyLimit::max(READING_ANALYZE_BODY_LIMIT_BYTES))
        .layer(RequestBodyLimitLayer::new(READING_ANALYZE_BODY_LIMIT_BYTES))
        .layer(middleware::from_fn(move |req, next| {
            rate_limit_middleware(reading_limiter.clone(), RateLimitTier::General, req, next)
        }));

    let general_limiter = state.rate_limiter.clone();
    let general_routes = Router::new()
        .route("/auth/providers", get(auth_providers))
        .route("/auth/me", get(auth_me))
        .route("/auth/logout", post(logout))
        .route("/account/profile", get(get_account_profile).put(update_account_profile))
        .route("/account/change-password", post(change_account_password))
        .route("/account/sessions", get(get_account_sessions))
        .route("/account/sessions/revoke-others", post(revoke_other_sessions))
        .route("/account/sessions/{id}", delete(revoke_single_session))
        .route("/dashboard", get(get_dashboard))
        .route("/analytics/study", get(get_study_analytics))
        .route("/vocab/bookmarks", get(get_vocab_bookmarks).post(save_vocab_bookmark))
        .route("/vocab/bookmarks/export", get(export_vocab_bookmarks))
        .route("/vocab/bookmarks/{id}", delete(delete_vocab_bookmark))
        .route("/content/version", get(get_curriculum_version))
        .route("/onboarding/placement", get(get_onboarding_placement))
        .route("/onboarding/complete", post(complete_onboarding))
        .route("/onboarding/reset", post(reset_onboarding))
        .route("/settings/srs", get(get_srs_settings).put(update_srs_settings))
        .route("/export/progress", get(export_progress))
        .route("/reviews", get(get_reviews))
        .route("/reviews/submit", post(submit_review))
        .route("/reviews/undo", post(undo_review))
        .route("/lessons/start", post(start_lesson))
        .route("/grammar", get(get_grammar_list))
        .route("/grammar/{id}", get(get_grammar_item))
        .route("/grammar/{id}/notes", put(update_grammar_notes))
        .route("/grammar/{id}/reset", post(reset_grammar_progress))
        .route("/reading/passages", get(get_reading_passages))
        .route("/audio/sentence/{id}", get(get_sentence_audio))
        .route("/audio/status", get(get_audio_status))
        .route("/conjugation", get(get_conjugation_items))
        .route("/wanikani/status", get(get_wanikani_status))
        .route("/wanikani/sync/status", get(get_wanikani_sync_status))
        .route("/wanikani/disconnect", post(disconnect_wanikani))
        .route("/wanikani/settings", put(update_wanikani_settings))
        .layer(DefaultBodyLimit::max(DEFAULT_BODY_LIMIT_BYTES))
        .layer(RequestBodyLimitLayer::new(DEFAULT_BODY_LIMIT_BYTES))
        .layer(middleware::from_fn(move |req, next| {
            rate_limit_middleware(general_limiter.clone(), RateLimitTier::General, req, next)
        }));

    Router::new()
        .merge(auth_public_routes)
        .merge(wk_sync_routes)
        .merge(large_body_routes)
        .merge(general_routes)
}

fn err_response(status: StatusCode, msg: impl Into<String>) -> (StatusCode, Json<serde_json::Value>) {
    (status, Json(json!({ "error": msg.into() })))
}

// ---------------------------------------------------------------------------
// Auth Routes
// ---------------------------------------------------------------------------

async fn register(
    State(state): State<AppState>,
    headers: HeaderMap,
    extensions: Extensions,
    Json(req): Json<RegisterRequest>,
) -> Response {
    let email = match normalize_and_validate_email(&req.email) {
        Ok(e) => e,
        Err(e) => return err_response(StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    };
    let username = match validate_username(&req.username) {
        Ok(u) => u,
        Err(e) => return err_response(StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    };
    if let Err(e) = validate_password(&req.password) {
        return err_response(StatusCode::BAD_REQUEST, e.to_string()).into_response();
    }

    let password_hash = match hash_password(req.password).await {
        Ok(h) => h,
        Err(e) => return err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    let user = match create_user(&state.pool, &email, &username, &password_hash).await {
        Ok(u) => u,
        Err(CreateUserError::EmailTaken) => {
            return err_response(StatusCode::CONFLICT, "An account with that email already exists")
                .into_response();
        }
        Err(CreateUserError::UsernameTaken) => {
            return err_response(StatusCode::CONFLICT, "That username is already taken")
                .into_response();
        }
        Err(CreateUserError::Other(e)) => {
            return err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
        }
    };

    // Soft email verification: create a 24h verification token and dispatch verification email
    // asynchronously while signing the user in immediately.
    let raw_verify_token = generate_random_token();
    let verify_token_hash = hash_session_token(&raw_verify_token);
    let verify_expires_at = Utc::now() + Duration::hours(24);
    if let Err(e) =
        create_email_verification_token(&state.pool, user.id, &verify_token_hash, verify_expires_at)
            .await
    {
        tracing::error!(error = %e, "Failed to persist email verification token on registration");
    } else {
        let verify_url = format!(
            "{}/verify-email?token={}",
            state.app_public_url.trim_end_matches('/'),
            raw_verify_token
        );
        let mailer = state.mailer.clone();
        let verify_msg = build_email_verification_email(&user.email, &user.username, &verify_url);
        tokio::spawn(async move {
            match mailer.send(verify_msg).await {
                Ok(res) => tracing::info!(
                    provider = res.provider,
                    message_id = ?res.message_id,
                    "Email verification email dispatched"
                ),
                Err(e) => tracing::warn!(
                    provider = mailer.provider_name(),
                    error = %e,
                    "Failed to dispatch email verification email"
                ),
            }
        });
    }

    let peer = extensions.get::<ConnectInfo<SocketAddr>>().map(|c| c.0);
    issue_session_response(&state, &headers, peer, user, StatusCode::CREATED).await
}

async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    extensions: Extensions,
    Json(req): Json<LoginRequest>,
) -> Response {
    let email = req.email.trim().to_lowercase();
    if email.is_empty() || req.password.is_empty() {
        return err_response(StatusCode::BAD_REQUEST, "Email and password are required")
            .into_response();
    }

    let found = match find_user_by_email(&state.pool, &email).await {
        Ok(v) => v,
        Err(e) => {
            return err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
        }
    };

    let Some((user, password_hash)) = found else {
        dummy_password_check(req.password).await;
        return err_response(StatusCode::UNAUTHORIZED, "Invalid email or password").into_response();
    };

    if !verify_password(req.password, password_hash).await {
        return err_response(StatusCode::UNAUTHORIZED, "Invalid email or password").into_response();
    }

    state.metrics.inc_auth_logins();
    let peer = extensions.get::<ConnectInfo<SocketAddr>>().map(|c| c.0);
    issue_session_response(&state, &headers, peer, user, StatusCode::OK).await
}

async fn issue_session_response(
    state: &AppState,
    headers: &HeaderMap,
    peer: Option<SocketAddr>,
    user: crate::models::AuthUserInfo,
    status: StatusCode,
) -> Response {
    let raw_session_token = generate_random_token();
    let session_id_hash = hash_session_token(&raw_session_token);
    let csrf_token = generate_random_token();
    let ttl_days = state.security.session_ttl_days;
    let max_age_secs = ttl_days * 24 * 60 * 60;
    let expires_at = Utc::now() + Duration::days(ttl_days);

    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.chars().take(256).collect::<String>());
    let ip_address = state.rate_limiter.extract_client_key(headers, peer);

    if let Err(e) = create_session(
        &state.pool,
        &session_id_hash,
        user.id,
        &csrf_token,
        expires_at,
        user_agent.as_deref(),
        Some(ip_address.as_str()),
    )
    .await
    {
        return err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
    }

    let cookies = match build_session_cookies(
        &raw_session_token,
        &csrf_token,
        max_age_secs,
        state.security.cookie_secure,
    ) {
        Ok(c) => c,
        Err(e) => {
            return err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
        }
    };

    let mut resp = (status, Json(AuthResponse { user, csrf_token })).into_response();
    let resp_headers = resp.headers_mut();
    for cookie in cookies {
        resp_headers.append(header::SET_COOKIE, cookie);
    }
    resp
}

async fn auth_providers(State(state): State<AppState>) -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(AuthProvidersResponse {
            google: state.google_oauth.is_enabled(),
        }),
    )
}

async fn google_oauth_start(State(state): State<AppState>) -> Response {
    if !state.google_oauth.is_enabled() {
        return Redirect::to("/?auth_error=google_not_configured").into_response();
    }

    let oauth_state = generate_random_token();
    let (verifier, challenge) = generate_pkce_pair();
    let auth_url = match build_google_authorize_url(&state.google_oauth, &oauth_state, &challenge) {
        Ok(url) => url,
        Err(e) => {
            tracing::error!("Failed to build Google authorize URL: {}", e);
            return Redirect::to("/?auth_error=google_failed").into_response();
        }
    };

    let state_cookie =
        match build_oauth_state_cookie(&oauth_state, &verifier, state.security.cookie_secure) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("Failed to build OAuth state cookie: {}", e);
                return Redirect::to("/?auth_error=google_failed").into_response();
            }
        };

    let mut resp = Redirect::temporary(&auth_url).into_response();
    resp.headers_mut().append(header::SET_COOKIE, state_cookie);
    resp
}

#[derive(Deserialize)]
struct GoogleCallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

fn redirect_with_cleared_oauth_cookie(target: &str, secure: bool) -> Response {
    let mut resp = Redirect::to(target).into_response();
    if let Ok(clear) = build_clear_oauth_state_cookie(secure) {
        resp.headers_mut().append(header::SET_COOKIE, clear);
    }
    resp
}

async fn google_oauth_callback(
    State(state): State<AppState>,
    headers: HeaderMap,
    extensions: Extensions,
    Query(q): Query<GoogleCallbackQuery>,
) -> Response {
    let secure = state.security.cookie_secure;

    if !state.google_oauth.is_enabled() {
        return redirect_with_cleared_oauth_cookie("/?auth_error=google_not_configured", secure);
    }

    if let Some(err) = q.error.as_deref() {
        tracing::info!(error = err, "Google OAuth authorization declined or failed");
        return redirect_with_cleared_oauth_cookie("/?auth_error=google_cancelled", secure);
    }

    let Some(code) = q.code.as_deref().filter(|s| !s.is_empty()) else {
        return redirect_with_cleared_oauth_cookie("/?auth_error=google_failed", secure);
    };
    let Some(callback_state) = q.state.as_deref().filter(|s| !s.is_empty()) else {
        return redirect_with_cleared_oauth_cookie("/?auth_error=invalid_state", secure);
    };

    let Some(state_cookie_val) = extract_cookie(&headers, OAUTH_STATE_COOKIE_NAME) else {
        return redirect_with_cleared_oauth_cookie("/?auth_error=invalid_state", secure);
    };
    let Some(code_verifier) = verify_oauth_state_cookie(&state_cookie_val, callback_state) else {
        return redirect_with_cleared_oauth_cookie("/?auth_error=invalid_state", secure);
    };

    let profile =
        match exchange_code_for_google_profile(&state.google_oauth, code, &code_verifier).await {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("Google OAuth token/profile exchange failed: {}", e);
                return redirect_with_cleared_oauth_cookie("/?auth_error=google_failed", secure);
            }
        };

    let (user, is_new_user) = match upsert_google_user(&state.pool, &profile).await {
        Ok(v) => v,
        Err(e) => {
            tracing::error!("Failed to upsert Google OAuth user: {}", e);
            return redirect_with_cleared_oauth_cookie("/?auth_error=google_failed", secure);
        }
    };

    if is_new_user {
        let mailer = state.mailer.clone();
        let welcome_msg = build_welcome_email(&user.email, &user.username, &state.app_public_url);
        tokio::spawn(async move {
            if let Err(e) = mailer.send(welcome_msg).await {
                tracing::warn!(error = %e, "Failed to dispatch welcome email for Google user");
            }
        });
    }

    let raw_session_token = generate_random_token();
    let session_id_hash = hash_session_token(&raw_session_token);
    let csrf_token = generate_random_token();
    let ttl_days = state.security.session_ttl_days;
    let max_age_secs = ttl_days * 24 * 60 * 60;
    let expires_at = Utc::now() + Duration::days(ttl_days);

    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.chars().take(256).collect::<String>());
    let peer = extensions.get::<ConnectInfo<SocketAddr>>().map(|c| c.0);
    let ip_address = state.rate_limiter.extract_client_key(&headers, peer);

    if let Err(e) = create_session(
        &state.pool,
        &session_id_hash,
        user.id,
        &csrf_token,
        expires_at,
        user_agent.as_deref(),
        Some(ip_address.as_str()),
    )
    .await
    {
        tracing::error!("Failed to create session after Google OAuth: {}", e);
        return redirect_with_cleared_oauth_cookie("/?auth_error=google_failed", secure);
    }

    state.metrics.inc_auth_logins();
    let mut resp = Redirect::to("/").into_response();
    let resp_headers = resp.headers_mut();
    if let Ok(clear) = build_clear_oauth_state_cookie(secure) {
        resp_headers.append(header::SET_COOKIE, clear);
    }
    if let Ok(cookies) = build_session_cookies(&raw_session_token, &csrf_token, max_age_secs, secure)
    {
        for cookie in cookies {
            resp_headers.append(header::SET_COOKIE, cookie);
        }
    }
    resp
}

async fn auth_me(auth: AuthUser) -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(AuthResponse {
            user: auth.user,
            csrf_token: auth.csrf_token,
        }),
    )
}

async fn logout(State(state): State<AppState>, auth: AuthUser, headers: HeaderMap) -> Response {
    if let Err(err) = verify_csrf(&headers, &auth) {
        return err.into_response();
    }

    if let Err(e) = delete_session(&state.pool, &auth.session_id_hash).await {
        return err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
    }

    let cookies = match build_clear_session_cookies(state.security.cookie_secure) {
        Ok(c) => c,
        Err(e) => {
            return err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
        }
    };

    let mut resp = (StatusCode::OK, Json(json!({ "ok": true }))).into_response();
    let resp_headers = resp.headers_mut();
    for cookie in cookies {
        resp_headers.append(header::SET_COOKIE, cookie);
    }
    resp
}

// ---------------------------------------------------------------------------
// Authenticated App Routes (all scoped to auth.user_id())
// ---------------------------------------------------------------------------

async fn get_dashboard(State(state): State<AppState>, auth: AuthUser) -> impl IntoResponse {
    match compute_dashboard(&state.pool, auth.user_id()).await {
        Ok(stats) => (StatusCode::OK, Json(json!(stats))),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[derive(Deserialize)]
struct ReviewQuery {
    mode: Option<String>,
    jlpt: Option<String>,
    limit: Option<usize>,
}

async fn get_reviews(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(q): Query<ReviewQuery>,
) -> impl IntoResponse {
    let mode = q.mode.as_deref().unwrap_or("review");
    let limit = q.limit.unwrap_or(20).clamp(1, 50);
    match build_review_queue(&state.pool, auth.user_id(), mode, q.jlpt.as_deref(), limit).await {
        Ok(items) => (StatusCode::OK, Json(json!(items))),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn submit_review(
    State(state): State<AppState>,
    auth: AuthUser,
    headers: HeaderMap,
    Json(req): Json<SubmitAnswerRequest>,
) -> impl IntoResponse {
    if let Err(err) = verify_csrf(&headers, &auth) {
        return err;
    }

    let (primary, alts, sentence_hint) = match get_sentence_answers(&state.pool, req.sentence_id).await {
        Ok(Some(v)) => v,
        Ok(None) => return err_response(StatusCode::NOT_FOUND, "Sentence not found"),
        Err(e) => return err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };

    let (prev_srs, prev_ghost) =
        match get_user_progress_stages(&state.pool, auth.user_id(), req.grammar_id).await {
            Ok(Some(v)) => v,
            Ok(None) => return err_response(StatusCode::NOT_FOUND, "Grammar point not found"),
            Err(e) => return err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        };

    let grade = grade_answer(&req.user_input, &primary, &alts, &sentence_hint);
    state.metrics.inc_reviews_submitted();
    let now = Utc::now();

    // Non-penalizing "almost right" nuance hint (shake input without recording a miss or altering SRS)
    if let GradeOutcome::AlmostRight { hint, category } = grade {
        let resp = SubmitAnswerResponse {
            correct: false,
            almost_right: true,
            nuance_hint: Some(hint.clone()),
            nuance_category: Some(category.to_string()),
            matched_answer: String::new(),
            expected_answer: primary,
            alt_answers: alts,
            previous_srs_stage: prev_srs,
            new_srs_stage: prev_srs,
            new_srs_label: srs_stage_label(prev_srs),
            next_review_at: None,
            previous_ghost_level: prev_ghost,
            new_ghost_level: prev_ghost,
            feedback: hint,
            review_log_id: None,
        };
        return (StatusCode::OK, Json(json!(resp)));
    }

    let (correct, matched_answer) = match grade {
        GradeOutcome::Correct { matched } => (true, matched),
        GradeOutcome::Incorrect | GradeOutcome::AlmostRight { .. } => (false, String::new()),
    };

    if req.mode == "cram" {
        let feedback = if correct {
            "Correct! (Cram mode does not alter SRS schedules)".to_string()
        } else {
            format!("Expected 「{}」. Keep drilling in Cram mode!", primary)
        };
        let resp = SubmitAnswerResponse {
            correct,
            almost_right: false,
            nuance_hint: None,
            nuance_category: None,
            matched_answer,
            expected_answer: primary,
            alt_answers: alts,
            previous_srs_stage: prev_srs,
            new_srs_stage: prev_srs,
            new_srs_label: srs_stage_label(prev_srs),
            next_review_at: None,
            previous_ghost_level: prev_ghost,
            new_ghost_level: prev_ghost,
            feedback,
            review_log_id: None,
        };
        return (StatusCode::OK, Json(json!(resp)));
    }

    let ghost_intensity = match get_user_srs_settings(&state.pool, auth.user_id()).await {
        Ok(s) => GhostIntensity::parse(&s.ghost_intensity),
        Err(_) => GhostIntensity::Standard,
    };

    let transition = compute_srs_transition_with_intensity(
        prev_srs,
        prev_ghost,
        req.is_ghost,
        correct,
        ghost_intensity,
        now,
    );
    let next_review_str = transition.next_review_at.map(|d| d.to_rfc3339());

    let log_id = match record_review_submission(
        &state.pool,
        auth.user_id(),
        req.grammar_id,
        &req.mode,
        req.is_ghost,
        correct,
        &transition,
        now,
    )
    .await
    {
        Ok(id) => Some(id),
        Err(e) => return err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };

    let feedback = if req.is_ghost {
        if correct {
            if transition.new_ghost_level == 0 {
                "Ghost exorcised! You mastered this weak point.".to_string()
            } else {
                format!("Ghost weakened -> Level {}!", transition.new_ghost_level)
            }
        } else {
            format!(
                "Expected 「{}」. Ghost remains active for extra practice.",
                primary
            )
        }
    } else if correct {
        format!("Promoted to {}!", srs_stage_label(transition.new_stage))
    } else {
        format!(
            "Expected 「{}」. Spawned a Level {} Ghost Review so you'll lock it in.",
            primary, transition.new_ghost_level
        )
    };

    let resp = SubmitAnswerResponse {
        correct,
        almost_right: false,
        nuance_hint: None,
        nuance_category: None,
        matched_answer,
        expected_answer: primary,
        alt_answers: alts,
        previous_srs_stage: prev_srs,
        new_srs_stage: transition.new_stage,
        new_srs_label: srs_stage_label(transition.new_stage),
        next_review_at: next_review_str,
        previous_ghost_level: prev_ghost,
        new_ghost_level: transition.new_ghost_level,
        feedback,
        review_log_id: log_id,
    };

    (StatusCode::OK, Json(json!(resp)))
}

async fn undo_review(
    State(state): State<AppState>,
    auth: AuthUser,
    headers: HeaderMap,
    Json(req): Json<UndoReviewRequest>,
) -> impl IntoResponse {
    if let Err(err) = verify_csrf(&headers, &auth) {
        return err;
    }

    let now = Utc::now();
    match db_undo_review(
        &state.pool,
        auth.user_id(),
        req.grammar_id,
        req.previous_srs_stage,
        req.previous_ghost_level,
        req.review_log_id,
        now,
    )
    .await
    {
        Ok(()) => (StatusCode::OK, Json(json!({ "ok": true }))),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn start_lesson(
    State(state): State<AppState>,
    auth: AuthUser,
    headers: HeaderMap,
    Json(req): Json<StartLessonRequest>,
) -> impl IntoResponse {
    if let Err(err) = verify_csrf(&headers, &auth) {
        return err;
    }

    let next_due = Utc::now() + Duration::hours(srs_interval_hours(1).unwrap_or(4));
    match db_start_lesson(&state.pool, auth.user_id(), req.grammar_id, next_due).await {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({ "ok": true, "next_review_at": next_due.to_rfc3339() })),
        ),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[derive(Deserialize)]
struct GrammarQuery {
    jlpt: Option<String>,
    q: Option<String>,
    status: Option<String>,
    path: Option<String>,
}

async fn get_grammar_list(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(q): Query<GrammarQuery>,
) -> impl IntoResponse {
    match list_grammar_summaries(
        &state.pool,
        auth.user_id(),
        q.jlpt.as_deref(),
        q.q.as_deref(),
        q.status.as_deref(),
        q.path.as_deref(),
    )
    .await
    {
        Ok(items) => (StatusCode::OK, Json(json!(items))),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn get_curriculum_version(
    State(state): State<AppState>,
    _auth: AuthUser,
) -> impl IntoResponse {
    match get_content_version(&state.pool).await {
        Ok(Some(v)) => (StatusCode::OK, Json(json!(v))),
        Ok(None) => err_response(StatusCode::NOT_FOUND, "Content version metadata not found"),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn get_onboarding_placement(
    State(state): State<AppState>,
    _auth: AuthUser,
) -> impl IntoResponse {
    match get_placement_questions(&state.pool).await {
        Ok(items) => (StatusCode::OK, Json(json!(items))),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn complete_onboarding(
    State(state): State<AppState>,
    auth: AuthUser,
    headers: HeaderMap,
    Json(req): Json<CompleteOnboardingRequest>,
) -> impl IntoResponse {
    if let Err(err) = verify_csrf(&headers, &auth) {
        return err;
    }

    match db_complete_onboarding(&state.pool, auth.user_id(), &req).await {
        Ok(onboarding) => (StatusCode::OK, Json(json!(onboarding))),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn reset_onboarding(
    State(state): State<AppState>,
    auth: AuthUser,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(err) = verify_csrf(&headers, &auth) {
        return err;
    }

    match db_reset_onboarding(&state.pool, auth.user_id()).await {
        Ok(onboarding) => (StatusCode::OK, Json(json!(onboarding))),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn get_srs_settings(State(state): State<AppState>, auth: AuthUser) -> impl IntoResponse {
    match get_user_srs_settings(&state.pool, auth.user_id()).await {
        Ok(s) => (StatusCode::OK, Json(json!(s))),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn update_srs_settings(
    State(state): State<AppState>,
    auth: AuthUser,
    headers: HeaderMap,
    Json(req): Json<UpdateSrsSettingsRequest>,
) -> impl IntoResponse {
    if let Err(err) = verify_csrf(&headers, &auth) {
        return err;
    }

    match db_update_srs_settings(&state.pool, auth.user_id(), &req).await {
        Ok(s) => (StatusCode::OK, Json(json!(s))),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[derive(Deserialize)]
struct ExportQuery {
    format: Option<String>,
}

async fn export_progress(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(q): Query<ExportQuery>,
) -> Response {
    let payload = match export_user_progress(&state.pool, auth.user_id()).await {
        Ok(p) => p,
        Err(e) => {
            return err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
        }
    };

    let fmt = q.format.as_deref().unwrap_or("json").to_ascii_lowercase();
    if fmt == "csv" {
        let csv = progress_export_to_csv(&payload);
        return (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, "text/csv; charset=utf-8"),
                (
                    header::CONTENT_DISPOSITION,
                    "attachment; filename=\"bunamateur-progress.csv\"",
                ),
            ],
            csv,
        )
            .into_response();
    }

    let body = serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".to_string());
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/json; charset=utf-8"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"bunamateur-progress.json\"",
            ),
        ],
        body,
    )
        .into_response()
}

async fn import_progress(
    State(state): State<AppState>,
    auth: AuthUser,
    headers: HeaderMap,
    Json(req): Json<ImportProgressRequest>,
) -> impl IntoResponse {
    if let Err(err) = verify_csrf(&headers, &auth) {
        return err;
    }

    match import_user_progress(&state.pool, auth.user_id(), &req.format, &req.data).await {
        Ok(summary) => (StatusCode::OK, Json(json!(summary))),
        Err(e) => err_response(StatusCode::BAD_REQUEST, e.to_string()),
    }
}

async fn get_grammar_item(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    match get_grammar_detail(&state.pool, auth.user_id(), id).await {
        Ok(Some(detail)) => (StatusCode::OK, Json(json!(detail))),
        Ok(None) => err_response(StatusCode::NOT_FOUND, "Grammar point not found"),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn update_grammar_notes(
    State(state): State<AppState>,
    auth: AuthUser,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(req): Json<UpdateNotesRequest>,
) -> impl IntoResponse {
    if let Err(err) = verify_csrf(&headers, &auth) {
        return err;
    }

    match db_update_grammar_notes(&state.pool, auth.user_id(), id, &req.notes).await {
        Ok(()) => (StatusCode::OK, Json(json!({ "ok": true }))),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn reset_grammar_progress(
    State(state): State<AppState>,
    auth: AuthUser,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    if let Err(err) = verify_csrf(&headers, &auth) {
        return err;
    }

    match db_reset_grammar_progress(&state.pool, auth.user_id(), id).await {
        Ok(()) => (StatusCode::OK, Json(json!({ "ok": true }))),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn get_reading_passages(
    State(state): State<AppState>,
    auth: AuthUser,
) -> impl IntoResponse {
    match list_reading_passages(&state.pool, auth.user_id()).await {
        Ok(items) => (StatusCode::OK, Json(json!(items))),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn analyze_reading(
    State(state): State<AppState>,
    auth: AuthUser,
    headers: HeaderMap,
    Json(req): Json<ReadingAnalyzeRequest>,
) -> impl IntoResponse {
    if let Err(err) = verify_csrf(&headers, &auth) {
        return err;
    }

    state.metrics.inc_nlp_analyses();
    match analyze_reading_text(&state.pool, auth.user_id(), &req.text).await {
        Ok(resp) => (StatusCode::OK, Json(json!(resp))),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn get_audio_status(State(state): State<AppState>, _auth: AuthUser) -> impl IntoResponse {
    (StatusCode::OK, Json(json!(state.audio.cache_status())))
}

async fn get_sentence_audio(
    State(state): State<AppState>,
    _auth: AuthUser,
    headers: HeaderMap,
    Path(sentence_id): Path<i64>,
) -> Response {
    state.metrics.inc_audio_requests();

    let info = match get_sentence_audio_info(&state.pool, sentence_id).await {
        Ok(v) => v,
        Err(e) => {
            return err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
        }
    };
    let Some((explicit_url, plain_text)) = info else {
        return err_response(StatusCode::NOT_FOUND, "Sentence not found").into_response();
    };

    match state
        .audio
        .resolve_sentence_audio(sentence_id, &explicit_url, &plain_text)
        .await
    {
        Ok(AudioResolution::Redirect(url)) => Redirect::temporary(&url).into_response(),
        Ok(AudioResolution::Bytes {
            bytes,
            mime_type,
            etag,
            source,
        }) => {
            if matches_if_none_match(&headers, &etag) {
                let mut resp = StatusCode::NOT_MODIFIED.into_response();
                if let Ok(val) = HeaderValue::from_str(&etag) {
                    resp.headers_mut().insert(header::ETAG, val);
                }
                resp.headers_mut().insert(
                    header::CACHE_CONTROL,
                    HeaderValue::from_static("public, max-age=31536000, immutable"),
                );
                return resp;
            }

            let mut resp = (StatusCode::OK, bytes).into_response();
            let resp_headers = resp.headers_mut();
            if let Ok(ct) = HeaderValue::from_str(&mime_type) {
                resp_headers.insert(header::CONTENT_TYPE, ct);
            }
            if let Ok(val) = HeaderValue::from_str(&etag) {
                resp_headers.insert(header::ETAG, val);
            }
            resp_headers.insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static("public, max-age=31536000, immutable"),
            );
            resp_headers.insert("x-audio-source", HeaderValue::from_static(source));
            resp
        }
        Ok(AudioResolution::NotAvailable) => err_response(
            StatusCode::NOT_FOUND,
            "No pre-generated sentence audio available; use client Web Speech fallback",
        )
        .into_response(),
        Err(e) => {
            tracing::warn!(sentence_id, error = %e, "Sentence audio synthesis/lookup failed");
            err_response(
                StatusCode::BAD_GATEWAY,
                "Sentence audio provider failed; use client Web Speech fallback",
            )
            .into_response()
        }
    }
}

#[derive(Deserialize)]
struct ConjugationQuery {
    form: Option<String>,
}

async fn get_conjugation_items(
    State(state): State<AppState>,
    _auth: AuthUser,
    Query(q): Query<ConjugationQuery>,
) -> impl IntoResponse {
    match list_conjugations(&state.pool, q.form.as_deref()).await {
        Ok(items) => (StatusCode::OK, Json(json!(items))),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn get_wanikani_status(State(state): State<AppState>, auth: AuthUser) -> impl IntoResponse {
    match load_wanikani_status(&state.pool, auth.user_id()).await {
        Ok(status) => (StatusCode::OK, Json(json!(status))),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn get_wanikani_sync_status(
    State(state): State<AppState>,
    auth: AuthUser,
) -> impl IntoResponse {
    match load_wanikani_sync_status(&state.pool, auth.user_id()).await {
        Ok(status) => (StatusCode::OK, Json(json!(status))),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn connect_wanikani(
    State(state): State<AppState>,
    auth: AuthUser,
    headers: HeaderMap,
    Json(req): Json<WaniKaniConnectRequest>,
) -> impl IntoResponse {
    if let Err(err) = verify_csrf(&headers, &auth) {
        return err;
    }

    let token = req.api_token.trim().to_string();
    if token.is_empty() {
        return err_response(StatusCode::BAD_REQUEST, "API token is required");
    }

    let client = match WaniKaniClient::new(state.wk_api_base_url.clone(), 500) {
        Ok(c) => c,
        Err(e) => return err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };

    let (username, level, user_etag) = match client.verify_token(&token).await {
        Ok(v) => v,
        Err(e) => return err_response(StatusCode::BAD_GATEWAY, e.to_string()),
    };

    let status = match save_wanikani_token_and_start_sync(
        &state.pool,
        &state.cipher,
        auth.user_id(),
        &token,
        &username,
        level,
        user_etag.as_deref(),
    )
    .await
    {
        Ok(s) => s,
        Err(e) => return err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };

    if let Err(e) = spawn_wanikani_sync_job(state.clone(), auth.user_id(), true).await {
        return err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
    }

    (StatusCode::OK, Json(json!(status)))
}

#[derive(Deserialize)]
struct WaniKaniSyncQuery {
    full: Option<bool>,
}

async fn resync_wanikani(
    State(state): State<AppState>,
    auth: AuthUser,
    headers: HeaderMap,
    Query(q): Query<WaniKaniSyncQuery>,
) -> impl IntoResponse {
    if let Err(err) = verify_csrf(&headers, &auth) {
        return err;
    }

    let token = match get_wanikani_credentials(&state.pool, &state.cipher, auth.user_id()).await {
        Ok(v) => v,
        Err(e) => return err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };

    if token.is_none() {
        return err_response(StatusCode::BAD_REQUEST, "WaniKani is not connected yet");
    }

    let force_full = q.full.unwrap_or(false);
    if let Err(e) = spawn_wanikani_sync_job(state.clone(), auth.user_id(), force_full).await {
        return err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
    }

    match load_wanikani_status(&state.pool, auth.user_id()).await {
        Ok(status) => (StatusCode::OK, Json(json!(status))),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn disconnect_wanikani(
    State(state): State<AppState>,
    auth: AuthUser,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(err) = verify_csrf(&headers, &auth) {
        return err;
    }

    match db_disconnect_wanikani(&state.pool, auth.user_id()).await {
        Ok(status) => (StatusCode::OK, Json(json!(status))),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn update_wanikani_settings(
    State(state): State<AppState>,
    auth: AuthUser,
    headers: HeaderMap,
    Json(req): Json<WaniKaniSettingsRequest>,
) -> impl IntoResponse {
    if let Err(err) = verify_csrf(&headers, &auth) {
        return err;
    }

    match db_update_wanikani_settings(
        &state.pool,
        auth.user_id(),
        req.smart_furigana,
        req.highlight_known_kanji,
        req.auto_sync_enabled,
        req.auto_sync_interval_mins,
    )
    .await
    {
        Ok(status) => (StatusCode::OK, Json(json!(status))),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

// ---------------------------------------------------------------------------
// v0.2 Email Verification, Password Reset, Account Security & Active Sessions
// ---------------------------------------------------------------------------

async fn verify_email(
    State(state): State<AppState>,
    Json(req): Json<VerifyEmailRequest>,
) -> impl IntoResponse {
    let raw_token = req.token.trim();
    if raw_token.len() < 16 {
        return err_response(StatusCode::BAD_REQUEST, "Invalid or expired verification token");
    }

    let token_hash = hash_session_token(raw_token);
    match consume_email_verification_token(&state.pool, &token_hash).await {
        Ok(Some(_)) => (
            StatusCode::OK,
            Json(json!({
                "ok": true,
                "message": "Your email address has been verified!"
            })),
        ),
        Ok(None) => err_response(
            StatusCode::BAD_REQUEST,
            "This verification link is invalid or has expired",
        ),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn resend_verification(
    State(state): State<AppState>,
    auth: AuthUser,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(err) = verify_csrf(&headers, &auth) {
        return err;
    }

    if auth.user.email_verified {
        return (
            StatusCode::OK,
            Json(json!({
                "ok": true,
                "already_verified": true,
                "message": "Your email address is already verified."
            })),
        );
    }

    let raw_token = generate_random_token();
    let token_hash = hash_session_token(&raw_token);
    let expires_at = Utc::now() + Duration::hours(24);

    if let Err(e) =
        create_email_verification_token(&state.pool, auth.user_id(), &token_hash, expires_at).await
    {
        return err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
    }

    let verify_url = format!(
        "{}/verify-email?token={}",
        state.app_public_url.trim_end_matches('/'),
        raw_token
    );
    let msg = build_email_verification_email(&auth.user.email, &auth.user.username, &verify_url);
    let mailer = state.mailer.clone();
    tokio::spawn(async move {
        match mailer.send(msg).await {
            Ok(res) => tracing::info!(
                provider = res.provider,
                message_id = ?res.message_id,
                "Verification email re-dispatched"
            ),
            Err(e) => tracing::warn!(
                provider = mailer.provider_name(),
                error = %e,
                "Failed to re-dispatch verification email"
            ),
        }
    });

    (
        StatusCode::OK,
        Json(json!({
            "ok": true,
            "already_verified": false,
            "message": "Verification email sent! Please check your inbox."
        })),
    )
}

async fn forgot_password(
    State(state): State<AppState>,
    Json(req): Json<ForgotPasswordRequest>,
) -> impl IntoResponse {
    const GENERIC_MSG: &str =
        "If an account matches that email, we have sent password reset instructions.";

    let Ok(email) = normalize_and_validate_email(&req.email) else {
        return (StatusCode::OK, Json(json!({ "ok": true, "message": GENERIC_MSG })));
    };

    if let Ok(Some((user, _))) = find_user_by_email(&state.pool, &email).await {
        let raw_token = generate_random_token();
        let token_hash = hash_session_token(&raw_token);
        let expires_at = Utc::now() + Duration::minutes(60);

        if let Err(e) =
            create_password_reset_token(&state.pool, user.id, &token_hash, expires_at).await
        {
            tracing::error!(error = %e, "Failed to persist password reset token");
        } else {
            let reset_url = format!(
                "{}/reset-password?token={}",
                state.app_public_url.trim_end_matches('/'),
                raw_token
            );
            let msg = build_password_reset_email(&user.email, &user.username, &reset_url);
            let mailer = state.mailer.clone();
            tokio::spawn(async move {
                match mailer.send(msg).await {
                    Ok(res) => tracing::info!(
                        provider = res.provider,
                        message_id = ?res.message_id,
                        "Password reset email dispatched"
                    ),
                    Err(e) => tracing::warn!(
                        provider = mailer.provider_name(),
                        error = %e,
                        "Failed to dispatch password reset email"
                    ),
                }
            });
        }
    }

    (StatusCode::OK, Json(json!({ "ok": true, "message": GENERIC_MSG })))
}

async fn reset_password(
    State(state): State<AppState>,
    Json(req): Json<ResetPasswordRequest>,
) -> impl IntoResponse {
    let raw_token = req.token.trim();
    if raw_token.len() < 16 {
        return err_response(StatusCode::BAD_REQUEST, "Invalid or expired password reset token");
    }
    if let Err(e) = validate_password(&req.new_password) {
        return err_response(StatusCode::BAD_REQUEST, e.to_string());
    }

    let new_hash = match hash_password(req.new_password).await {
        Ok(h) => h,
        Err(e) => return err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    let token_hash = hash_session_token(raw_token);

    match consume_password_reset_token(&state.pool, &token_hash, &new_hash).await {
        Ok(Some(_)) => (
            StatusCode::OK,
            Json(json!({
                "ok": true,
                "message": "Your password has been reset. Please sign in with your new password."
            })),
        ),
        Ok(None) => err_response(
            StatusCode::BAD_REQUEST,
            "This password reset link is invalid or has expired",
        ),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn get_account_profile(State(state): State<AppState>, auth: AuthUser) -> impl IntoResponse {
    match db_get_account_profile(&state.pool, auth.user_id()).await {
        Ok(Some(profile)) => (StatusCode::OK, Json(json!(profile))),
        Ok(None) => err_response(StatusCode::NOT_FOUND, "Account not found"),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn update_account_profile(
    State(state): State<AppState>,
    auth: AuthUser,
    headers: HeaderMap,
    Json(req): Json<UpdateProfileRequest>,
) -> impl IntoResponse {
    if let Err(err) = verify_csrf(&headers, &auth) {
        return err;
    }

    let username = match validate_username(&req.username) {
        Ok(u) => u,
        Err(e) => return err_response(StatusCode::BAD_REQUEST, e.to_string()),
    };

    match update_user_username(&state.pool, auth.user_id(), &username).await {
        Ok(profile) => (StatusCode::OK, Json(json!(profile))),
        Err(CreateUserError::UsernameTaken) => {
            err_response(StatusCode::CONFLICT, "That username is already taken")
        }
        Err(CreateUserError::EmailTaken) => {
            err_response(StatusCode::CONFLICT, "Email conflict")
        }
        Err(CreateUserError::Other(e)) => {
            err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
        }
    }
}

async fn change_account_password(
    State(state): State<AppState>,
    auth: AuthUser,
    headers: HeaderMap,
    Json(req): Json<ChangePasswordRequest>,
) -> impl IntoResponse {
    if let Err(err) = verify_csrf(&headers, &auth) {
        return err;
    }

    if let Err(e) = validate_password(&req.new_password) {
        return err_response(StatusCode::BAD_REQUEST, e.to_string());
    }

    let existing_hash = match get_user_password_hash(&state.pool, auth.user_id()).await {
        Ok(Some(h)) => h,
        Ok(None) => return err_response(StatusCode::NOT_FOUND, "Account not found"),
        Err(e) => return err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };

    let has_password = !existing_hash.starts_with('!');
    if has_password {
        let Some(cur_pw) = req.current_password.filter(|s| !s.is_empty()) else {
            return err_response(StatusCode::BAD_REQUEST, "Current password is required");
        };
        if !verify_password(cur_pw, existing_hash).await {
            return err_response(StatusCode::UNAUTHORIZED, "Current password is incorrect");
        }
    }

    let new_hash = match hash_password(req.new_password).await {
        Ok(h) => h,
        Err(e) => return err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };

    let keep_session = if req.revoke_other_sessions.unwrap_or(true) {
        Some(auth.session_id_hash.as_str())
    } else {
        None
    };

    match update_user_password_hash(&state.pool, auth.user_id(), &new_hash, keep_session).await {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({
                "ok": true,
                "message": "Password updated successfully."
            })),
        ),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn get_account_sessions(State(state): State<AppState>, auth: AuthUser) -> impl IntoResponse {
    match list_user_sessions(&state.pool, auth.user_id(), &auth.session_id_hash).await {
        Ok(list) => (StatusCode::OK, Json(json!(list))),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn revoke_single_session(
    State(state): State<AppState>,
    auth: AuthUser,
    headers: HeaderMap,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    if let Err(err) = verify_csrf(&headers, &auth) {
        return err;
    }

    if session_id == auth.session_id_hash {
        return err_response(
            StatusCode::BAD_REQUEST,
            "Use Sign Out to terminate your current session",
        );
    }

    match revoke_user_session(&state.pool, auth.user_id(), &session_id).await {
        Ok(true) => (StatusCode::OK, Json(json!({ "ok": true }))),
        Ok(false) => err_response(StatusCode::NOT_FOUND, "Session not found"),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn revoke_other_sessions(
    State(state): State<AppState>,
    auth: AuthUser,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(err) = verify_csrf(&headers, &auth) {
        return err;
    }

    match revoke_other_user_sessions(&state.pool, auth.user_id(), &auth.session_id_hash).await {
        Ok(revoked) => (StatusCode::OK, Json(json!({ "ok": true, "revoked": revoked }))),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

// ---------------------------------------------------------------------------
// v0.2 Study Analytics & Reading Lab Vocab Deck
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct AnalyticsQuery {
    days: Option<i32>,
}

async fn get_study_analytics(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(q): Query<AnalyticsQuery>,
) -> impl IntoResponse {
    let days = q.days.unwrap_or(91);
    match compute_study_analytics(&state.pool, auth.user_id(), days).await {
        Ok(analytics) => (StatusCode::OK, Json(json!(analytics))),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn get_vocab_bookmarks(State(state): State<AppState>, auth: AuthUser) -> impl IntoResponse {
    match list_vocab_bookmarks(&state.pool, auth.user_id()).await {
        Ok(items) => (StatusCode::OK, Json(json!(items))),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn save_vocab_bookmark(
    State(state): State<AppState>,
    auth: AuthUser,
    headers: HeaderMap,
    Json(req): Json<SaveVocabBookmarkRequest>,
) -> impl IntoResponse {
    if let Err(err) = verify_csrf(&headers, &auth) {
        return err;
    }

    match upsert_vocab_bookmark(&state.pool, auth.user_id(), &req).await {
        Ok(item) => (StatusCode::OK, Json(json!(item))),
        Err(e) => err_response(StatusCode::BAD_REQUEST, e.to_string()),
    }
}

async fn delete_vocab_bookmark(
    State(state): State<AppState>,
    auth: AuthUser,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    if let Err(err) = verify_csrf(&headers, &auth) {
        return err;
    }

    match db_delete_vocab_bookmark(&state.pool, auth.user_id(), id).await {
        Ok(true) => (StatusCode::OK, Json(json!({ "ok": true }))),
        Ok(false) => err_response(StatusCode::NOT_FOUND, "Bookmark not found"),
        Err(e) => err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[derive(Deserialize)]
struct VocabExportQuery {
    format: Option<String>,
}

async fn export_vocab_bookmarks(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(q): Query<VocabExportQuery>,
) -> Response {
    let items = match list_vocab_bookmarks(&state.pool, auth.user_id()).await {
        Ok(v) => v,
        Err(e) => {
            return err_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
        }
    };

    let fmt = q.format.as_deref().unwrap_or("anki").to_ascii_lowercase();
    if fmt == "json" {
        let body = serde_json::to_string_pretty(&items).unwrap_or_else(|_| "[]".to_string());
        return (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, "application/json; charset=utf-8"),
                (
                    header::CONTENT_DISPOSITION,
                    "attachment; filename=\"bunamateur-vocab.json\"",
                ),
            ],
            body,
        )
            .into_response();
    }

    let tsv = vocab_bookmarks_to_anki_tsv(&items);
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "text/tab-separated-values; charset=utf-8"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"bunamateur-vocab-anki.tsv\"",
            ),
        ],
        tsv,
    )
        .into_response()
}

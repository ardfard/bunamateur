use crate::audio::{AudioConfig, AudioService};
use crate::crypto::TokenCipher;
use crate::email::{
    DEFAULT_APP_PUBLIC_URL, DEFAULT_EMAIL_FROM, DynEmailSender, NoopLogEmailSender,
};
use crate::models::{
    AuthUserInfo, CompleteOnboardingRequest, ConjugationItem, ContentVersionInfo, DashboardStats,
    DetectedGrammar, ForecastBucket, GrammarPointDetail, GrammarPointSummary, GrammarSentence,
    ImportProgressResponse, JlptProgress, OnboardingState, PlacementQuestion,
    ProgressExportPayload, ProgressExportRow, ReadingAnalyzeResponse, ReadingPassage,
    ReviewQueueItem, SrsSettings, SrsTierCount, UpdateSrsSettingsRequest, WaniKaniStatus,
    WaniKaniSyncStatus,
};
use crate::nlp::{
    NlpAnalysis, WkSubjectLookup, analyze_japanese_text, match_pattern_against_analysis,
};
use crate::oauth::{GoogleOAuthConfig, GoogleProfile, derive_username_base};
use crate::observability::MetricsRegistry;
use crate::security::{RateLimiter, SecurityConfig};
use crate::seed_data::{SEED_CONJUGATIONS, SEED_PASSAGES};
use crate::seed_data_extra::{CONTENT_SCHEMA_VERSION, all_seed_grammar, sentences_for_grammar};
use crate::srs::{GhostIntensity, SrsTransition, srs_interval_hours, srs_stage_label};
use crate::wanikani::{
    DEFAULT_WK_API_BASE, WaniKaniClient, WaniKaniSyncCursor, WaniKaniSyncResult,
    compute_kanji_readiness, strip_ruby_markup,
};
use anyhow::{Context, Result};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use sha2::{Digest, Sha256};
use sqlx::{FromRow, PgPool, postgres::PgPoolOptions};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration as StdDuration;

pub const DEFAULT_DATABASE_URL: &str = "postgres://ardfard@127.0.0.1:5432/bunamateur_dev";

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub cipher: TokenCipher,
    pub security: SecurityConfig,
    pub rate_limiter: RateLimiter,
    pub mailer: DynEmailSender,
    pub app_public_url: String,
    pub google_oauth: GoogleOAuthConfig,
    pub wk_api_base_url: String,
    pub wk_sync_inflight: Arc<tokio::sync::Mutex<HashSet<i64>>>,
    pub audio: AudioService,
    pub metrics: MetricsRegistry,
}

impl AppState {
    pub async fn connect(database_url: &str) -> Result<Self> {
        let cipher = TokenCipher::from_env()?;
        let security = SecurityConfig::default();
        Self::connect_with_config(database_url, cipher, security).await
    }

    pub async fn connect_with_config(
        database_url: &str,
        cipher: TokenCipher,
        security: SecurityConfig,
    ) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(10)
            .acquire_timeout(StdDuration::from_secs(10))
            .connect(database_url)
            .await
            .with_context(|| {
                format!(
                    "Failed to connect to PostgreSQL ({})",
                    sanitize_database_url(database_url)
                )
            })?;

        Self::from_pool_with_config(pool, cipher, security).await
    }

    pub async fn from_pool(pool: PgPool) -> Result<Self> {
        Self::from_pool_with_config(
            pool,
            TokenCipher::generate_ephemeral(),
            SecurityConfig::default(),
        )
        .await
    }

    pub async fn from_pool_with_config(
        pool: PgPool,
        cipher: TokenCipher,
        security: SecurityConfig,
    ) -> Result<Self> {
        run_migrations(&pool).await?;
        seed_content(&pool).await?;
        let rate_limiter = RateLimiter::new(security.rate_limits.clone(), security.trust_proxy);
        let mailer: DynEmailSender =
            Arc::new(NoopLogEmailSender::new(DEFAULT_EMAIL_FROM.to_string()));
        let wk_api_base_url = std::env::var("WANIKANI_API_BASE_URL")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_WK_API_BASE.to_string());
        Ok(Self {
            pool,
            cipher,
            security,
            rate_limiter,
            mailer,
            app_public_url: DEFAULT_APP_PUBLIC_URL.to_string(),
            google_oauth: GoogleOAuthConfig::default(),
            wk_api_base_url,
            wk_sync_inflight: Arc::new(tokio::sync::Mutex::new(HashSet::new())),
            audio: AudioService::new(AudioConfig::from_env()),
            metrics: MetricsRegistry::new(),
        })
    }

    pub fn with_mailer(mut self, mailer: DynEmailSender, app_public_url: String) -> Self {
        self.mailer = mailer;
        self.app_public_url = app_public_url;
        self
    }

    pub fn with_google_oauth(mut self, google_oauth: GoogleOAuthConfig) -> Self {
        self.google_oauth = google_oauth;
        self
    }

    pub fn with_wk_api_base_url(mut self, url: String) -> Self {
        self.wk_api_base_url = url;
        self
    }

    pub fn with_audio(mut self, audio: AudioService) -> Self {
        self.audio = audio;
        self
    }

    pub fn with_metrics(mut self, metrics: MetricsRegistry) -> Self {
        self.metrics = metrics;
        self
    }
}

pub async fn ping_database(pool: &PgPool) -> Result<()> {
    let _: i32 = sqlx::query_scalar("SELECT 1").fetch_one(pool).await?;
    Ok(())
}

/// Redacts any password embedded in a PostgreSQL connection URL before logging.
pub fn sanitize_database_url(url: &str) -> String {
    if let Some((scheme, rest)) = url.split_once("://")
        && let Some((userinfo, host_and_path)) = rest.split_once('@')
    {
        let safe_user = userinfo.split(':').next().unwrap_or("");
        if safe_user.is_empty() {
            return format!("{}://{}", scheme, host_and_path);
        }
        return format!("{}://{}@{}", scheme, safe_user, host_and_path);
    }
    url.to_string()
}

pub async fn run_migrations(pool: &PgPool) -> Result<()> {
    sqlx::migrate!("./migrations")
        .run(pool)
        .await
        .context("Failed to run embedded PostgreSQL migrations")?;
    Ok(())
}

/// Computes a deterministic SHA-256 checksum for the embedded curriculum catalog.
pub fn compute_curriculum_checksum() -> (String, i32, i32) {
    let mut hasher = Sha256::new();
    let all = all_seed_grammar();
    let mut total_sentences: i32 = 0;

    hasher.update(CONTENT_SCHEMA_VERSION.to_le_bytes());
    for g in &all {
        hasher.update(g.slug.as_bytes());
        hasher.update(b"|");
        hasher.update(g.title.as_bytes());
        hasher.update(b"|");
        hasher.update(g.meaning.as_bytes());
        hasher.update(b"|");
        hasher.update(g.jlpt_level.as_bytes());
        hasher.update(b"|");
        hasher.update(g.study_path.as_bytes());
        for s in sentences_for_grammar(g) {
            total_sentences += 1;
            hasher.update(s.jp_before.as_bytes());
            hasher.update(s.answer.as_bytes());
            hasher.update(s.jp_after.as_bytes());
            hasher.update(s.hint.as_bytes());
        }
    }
    for p in SEED_PASSAGES {
        hasher.update(p.title.as_bytes());
        hasher.update(p.content_ruby.as_bytes());
    }
    for c in SEED_CONJUGATIONS {
        hasher.update(c.dictionary.as_bytes());
        hasher.update(c.target_form.as_bytes());
    }

    (hex::encode(hasher.finalize()), all.len() as i32, total_sentences)
}

/// Idempotently upserts shared static curriculum content (grammar points, sentences,
/// reading passages, conjugation drills) with version & checksum tracking, without
/// touching any user-scoped tables.
pub async fn seed_content(pool: &PgPool) -> Result<()> {
    let all_grammar = all_seed_grammar();
    let (checksum, item_count, sentence_count) = compute_curriculum_checksum();

    let mut tx = pool.begin().await?;

    for g in all_grammar {
        let patterns: Vec<String> = g.match_patterns.iter().map(|s| (*s).to_string()).collect();

        let grammar_id: i64 = sqlx::query_scalar(
            "INSERT INTO grammar_points
                 (slug, title, reading, meaning, jlpt_level, study_path, structure, nuance, caution, confusable_with, pitch_note, match_patterns, content_version)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
             ON CONFLICT (slug) DO UPDATE SET
                 title = EXCLUDED.title,
                 reading = EXCLUDED.reading,
                 meaning = EXCLUDED.meaning,
                 jlpt_level = EXCLUDED.jlpt_level,
                 study_path = EXCLUDED.study_path,
                 structure = EXCLUDED.structure,
                 nuance = EXCLUDED.nuance,
                 caution = EXCLUDED.caution,
                 confusable_with = EXCLUDED.confusable_with,
                 pitch_note = EXCLUDED.pitch_note,
                 match_patterns = EXCLUDED.match_patterns,
                 content_version = EXCLUDED.content_version
             RETURNING id",
        )
        .bind(g.slug)
        .bind(g.title)
        .bind(g.reading)
        .bind(g.meaning)
        .bind(g.jlpt_level)
        .bind(g.study_path)
        .bind(g.structure)
        .bind(g.nuance)
        .bind(g.caution)
        .bind(g.confusable_with)
        .bind(g.pitch_note)
        .bind(&patterns)
        .bind(CONTENT_SCHEMA_VERSION)
        .fetch_one(&mut *tx)
        .await?;

        let sentences = sentences_for_grammar(g);
        for (pos, s) in sentences.iter().enumerate() {
            let alts: Vec<String> = s.alt_answers.iter().map(|a| (*a).to_string()).collect();
            sqlx::query(
                "INSERT INTO grammar_sentences
                     (grammar_id, position, jp_before, jp_after, answer, alt_answers, hint, english, nuance_note, content_version)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
                 ON CONFLICT (grammar_id, position) DO UPDATE SET
                     jp_before = EXCLUDED.jp_before,
                     jp_after = EXCLUDED.jp_after,
                     answer = EXCLUDED.answer,
                     alt_answers = EXCLUDED.alt_answers,
                     hint = EXCLUDED.hint,
                     english = EXCLUDED.english,
                     nuance_note = EXCLUDED.nuance_note,
                     content_version = EXCLUDED.content_version",
            )
            .bind(grammar_id)
            .bind(pos as i32)
            .bind(s.jp_before)
            .bind(s.jp_after)
            .bind(s.answer)
            .bind(&alts)
            .bind(s.hint)
            .bind(s.english)
            .bind(s.nuance_note)
            .bind(CONTENT_SCHEMA_VERSION)
            .execute(&mut *tx)
            .await?;
        }
    }

    for p in SEED_PASSAGES {
        sqlx::query(
            "INSERT INTO reading_passages (title, jlpt_level, topic, content_ruby, english)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (title) DO UPDATE SET
                 jlpt_level = EXCLUDED.jlpt_level,
                 topic = EXCLUDED.topic,
                 content_ruby = EXCLUDED.content_ruby,
                 english = EXCLUDED.english",
        )
        .bind(p.title)
        .bind(p.jlpt_level)
        .bind(p.topic)
        .bind(p.content_ruby)
        .bind(p.english)
        .execute(&mut *tx)
        .await?;
    }

    for c in SEED_CONJUGATIONS {
        sqlx::query(
            "INSERT INTO conjugation_items
                 (dictionary, reading, meaning, word_type, target_form, target_label, answer_kana, answer_kanji, rule_explanation, example_jp, example_en)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
             ON CONFLICT (dictionary, target_form) DO UPDATE SET
                 reading = EXCLUDED.reading,
                 meaning = EXCLUDED.meaning,
                 word_type = EXCLUDED.word_type,
                 target_label = EXCLUDED.target_label,
                 answer_kana = EXCLUDED.answer_kana,
                 answer_kanji = EXCLUDED.answer_kanji,
                 rule_explanation = EXCLUDED.rule_explanation,
                 example_jp = EXCLUDED.example_jp,
                 example_en = EXCLUDED.example_en",
        )
        .bind(c.dictionary)
        .bind(c.reading)
        .bind(c.meaning)
        .bind(c.word_type)
        .bind(c.target_form)
        .bind(c.target_label)
        .bind(c.answer_kana)
        .bind(c.answer_kanji)
        .bind(c.rule_explanation)
        .bind(c.example_jp)
        .bind(c.example_en)
        .execute(&mut *tx)
        .await?;
    }

    sqlx::query(
        "INSERT INTO content_versions (content_key, version, checksum, item_count, sentence_count, applied_at)
         VALUES ('curriculum', $1, $2, $3, $4, NOW())
         ON CONFLICT (content_key) DO UPDATE SET
             version = EXCLUDED.version,
             checksum = EXCLUDED.checksum,
             item_count = EXCLUDED.item_count,
             sentence_count = EXCLUDED.sentence_count,
             applied_at = NOW()",
    )
    .bind(CONTENT_SCHEMA_VERSION)
    .bind(&checksum)
    .bind(item_count)
    .bind(sentence_count)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(())
}

#[derive(FromRow)]
struct ContentVersionRow {
    content_key: String,
    version: i32,
    checksum: String,
    item_count: i32,
    sentence_count: i32,
    applied_at: DateTime<Utc>,
}

pub async fn get_content_version(pool: &PgPool) -> Result<Option<ContentVersionInfo>> {
    let row: Option<ContentVersionRow> = sqlx::query_as(
        "SELECT content_key, version, checksum, item_count, sentence_count, applied_at
         FROM content_versions
         WHERE content_key = 'curriculum'",
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| ContentVersionInfo {
        content_key: r.content_key,
        version: r.version,
        checksum: r.checksum,
        item_count: r.item_count,
        sentence_count: r.sentence_count,
        applied_at: r.applied_at.to_rfc3339(),
    }))
}

// ---------------------------------------------------------------------------
// Users & Sessions
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum CreateUserError {
    EmailTaken,
    UsernameTaken,
    Other(anyhow::Error),
}

#[derive(FromRow)]
struct UserRow {
    id: i64,
    email: String,
    username: String,
    avatar_url: Option<String>,
    created_at: DateTime<Utc>,
}

impl From<UserRow> for AuthUserInfo {
    fn from(r: UserRow) -> Self {
        Self {
            id: r.id,
            email: r.email,
            username: r.username,
            avatar_url: r.avatar_url,
            created_at: r.created_at.to_rfc3339(),
        }
    }
}

pub async fn create_user(
    pool: &PgPool,
    email: &str,
    username: &str,
    password_hash: &str,
) -> std::result::Result<AuthUserInfo, CreateUserError> {
    let mut tx = pool.begin().await.map_err(|e| CreateUserError::Other(e.into()))?;

    let row: UserRow = match sqlx::query_as(
        "INSERT INTO users (email, username, password_hash)
         VALUES ($1, $2, $3)
         RETURNING id, email, username, avatar_url, created_at",
    )
    .bind(email)
    .bind(username)
    .bind(password_hash)
    .fetch_one(&mut *tx)
    .await
    {
        Ok(r) => r,
        Err(sqlx::Error::Database(db_err)) if db_err.code().as_deref() == Some("23505") => {
            let constraint = db_err.constraint().unwrap_or("");
            if constraint.contains("username") {
                return Err(CreateUserError::UsernameTaken);
            }
            return Err(CreateUserError::EmailTaken);
        }
        Err(e) => return Err(CreateUserError::Other(e.into())),
    };

    sqlx::query(
        "INSERT INTO wk_profile (user_id, smart_furigana, highlight_known_kanji)
         VALUES ($1, TRUE, TRUE)
         ON CONFLICT (user_id) DO NOTHING",
    )
    .bind(row.id)
    .execute(&mut *tx)
    .await
    .map_err(|e| CreateUserError::Other(e.into()))?;

    tx.commit()
        .await
        .map_err(|e| CreateUserError::Other(e.into()))?;

    Ok(row.into())
}

#[derive(FromRow)]
struct UserWithPasswordRow {
    id: i64,
    email: String,
    username: String,
    avatar_url: Option<String>,
    password_hash: String,
    created_at: DateTime<Utc>,
}

pub async fn find_user_by_email(
    pool: &PgPool,
    email: &str,
) -> Result<Option<(AuthUserInfo, String)>> {
    let row: Option<UserWithPasswordRow> = sqlx::query_as(
        "SELECT id, email, username, avatar_url, password_hash, created_at
         FROM users
         WHERE LOWER(email) = LOWER($1)",
    )
    .bind(email)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| {
        (
            AuthUserInfo {
                id: r.id,
                email: r.email,
                username: r.username,
                avatar_url: r.avatar_url,
                created_at: r.created_at.to_rfc3339(),
            },
            r.password_hash,
        )
    }))
}

/// Finds, links, or creates a user from a verified Google OIDC profile.
/// Returns `(user, is_new_user)`.
pub async fn upsert_google_user(
    pool: &PgPool,
    profile: &GoogleProfile,
) -> Result<(AuthUserInfo, bool)> {
    let mut tx = pool.begin().await?;

    // 1. Existing linked Google identity
    let existing_linked: Option<UserRow> = sqlx::query_as(
        "SELECT u.id, u.email, u.username, u.avatar_url, u.created_at
         FROM oauth_identities o
         JOIN users u ON u.id = o.user_id
         WHERE o.provider = 'google' AND o.provider_user_id = $1",
    )
    .bind(&profile.sub)
    .fetch_optional(&mut *tx)
    .await?;

    if let Some(mut user) = existing_linked {
        sqlx::query(
            "UPDATE oauth_identities
             SET email = $2, avatar_url = $3, updated_at = NOW()
             WHERE provider = 'google' AND provider_user_id = $1",
        )
        .bind(&profile.sub)
        .bind(&profile.email)
        .bind(profile.picture.as_deref())
        .execute(&mut *tx)
        .await?;

        if let Some(pic) = &profile.picture {
            sqlx::query(
                "UPDATE users SET avatar_url = COALESCE(avatar_url, $2), updated_at = NOW() WHERE id = $1",
            )
            .bind(user.id)
            .bind(pic)
            .execute(&mut *tx)
            .await?;
            if user.avatar_url.is_none() {
                user.avatar_url = Some(pic.clone());
            }
        }

        tx.commit().await?;
        return Ok((user.into(), false));
    }

    // 2. Existing user with matching verified email -> link Google identity seamlessly
    let existing_by_email: Option<UserRow> = sqlx::query_as(
        "SELECT id, email, username, avatar_url, created_at
         FROM users
         WHERE LOWER(email) = LOWER($1)",
    )
    .bind(&profile.email)
    .fetch_optional(&mut *tx)
    .await?;

    if let Some(mut user) = existing_by_email {
        sqlx::query(
            "INSERT INTO oauth_identities (user_id, provider, provider_user_id, email, avatar_url)
             VALUES ($1, 'google', $2, $3, $4)
             ON CONFLICT (user_id, provider) DO UPDATE SET
                 provider_user_id = EXCLUDED.provider_user_id,
                 email = EXCLUDED.email,
                 avatar_url = EXCLUDED.avatar_url,
                 updated_at = NOW()",
        )
        .bind(user.id)
        .bind(&profile.sub)
        .bind(&profile.email)
        .bind(profile.picture.as_deref())
        .execute(&mut *tx)
        .await?;

        if let Some(pic) = &profile.picture
            && user.avatar_url.is_none()
        {
            sqlx::query("UPDATE users SET avatar_url = $2, updated_at = NOW() WHERE id = $1")
                .bind(user.id)
                .bind(pic)
                .execute(&mut *tx)
                .await?;
            user.avatar_url = Some(pic.clone());
        }

        tx.commit().await?;
        return Ok((user.into(), false));
    }

    // 3. Brand-new Google user: pick a unique sanitized username
    let base_username = derive_username_base(profile.name.as_deref(), &profile.email);
    let mut chosen_username = base_username.clone();
    for attempt in 0..10u32 {
        let taken: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM users WHERE LOWER(username) = LOWER($1))",
        )
        .bind(&chosen_username)
        .fetch_one(&mut *tx)
        .await?;
        if !taken {
            break;
        }
        let mut suffix_bytes = [0u8; 3];
        rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut suffix_bytes);
        let suffix = hex::encode(suffix_bytes);
        chosen_username = format!("{}_{}", base_username, &suffix[..4 + (attempt > 3) as usize]);
    }

    let created: UserRow = sqlx::query_as(
        "INSERT INTO users (email, username, password_hash, avatar_url)
         VALUES ($1, $2, '!oauth-google', $3)
         RETURNING id, email, username, avatar_url, created_at",
    )
    .bind(&profile.email)
    .bind(&chosen_username)
    .bind(profile.picture.as_deref())
    .fetch_one(&mut *tx)
    .await?;

    sqlx::query(
        "INSERT INTO wk_profile (user_id, smart_furigana, highlight_known_kanji)
         VALUES ($1, TRUE, TRUE)
         ON CONFLICT (user_id) DO NOTHING",
    )
    .bind(created.id)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "INSERT INTO oauth_identities (user_id, provider, provider_user_id, email, avatar_url)
         VALUES ($1, 'google', $2, $3, $4)",
    )
    .bind(created.id)
    .bind(&profile.sub)
    .bind(&profile.email)
    .bind(profile.picture.as_deref())
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok((created.into(), true))
}

pub async fn create_session(
    pool: &PgPool,
    session_id_hash: &str,
    user_id: i64,
    csrf_token: &str,
    expires_at: DateTime<Utc>,
    user_agent: Option<&str>,
    ip_address: Option<&str>,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO sessions (id, user_id, csrf_token, expires_at, user_agent, ip_address)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(session_id_hash)
    .bind(user_id)
    .bind(csrf_token)
    .bind(expires_at)
    .bind(user_agent)
    .bind(ip_address)
    .execute(pool)
    .await?;

    // Best-effort cleanup of expired sessions for this user
    let _ = sqlx::query("DELETE FROM sessions WHERE user_id = $1 AND expires_at <= NOW()")
        .bind(user_id)
        .execute(pool)
        .await;

    Ok(())
}

#[derive(FromRow)]
struct SessionUserRow {
    id: i64,
    email: String,
    username: String,
    avatar_url: Option<String>,
    created_at: DateTime<Utc>,
    csrf_token: String,
    last_seen_at: DateTime<Utc>,
}

pub async fn get_session_user(
    pool: &PgPool,
    session_id_hash: &str,
) -> Result<Option<(AuthUserInfo, String)>> {
    let row: Option<SessionUserRow> = sqlx::query_as(
        "SELECT u.id, u.email, u.username, u.avatar_url, u.created_at, s.csrf_token, s.last_seen_at
         FROM sessions s
         JOIN users u ON u.id = s.user_id
         WHERE s.id = $1 AND s.expires_at > NOW()",
    )
    .bind(session_id_hash)
    .fetch_optional(pool)
    .await?;

    let Some(row) = row else {
        return Ok(None);
    };

    if Utc::now() - row.last_seen_at > Duration::minutes(5) {
        let _ = sqlx::query("UPDATE sessions SET last_seen_at = NOW() WHERE id = $1")
            .bind(session_id_hash)
            .execute(pool)
            .await;
    }

    Ok(Some((
        AuthUserInfo {
            id: row.id,
            email: row.email,
            username: row.username,
            avatar_url: row.avatar_url,
            created_at: row.created_at.to_rfc3339(),
        },
        row.csrf_token,
    )))
}

pub async fn delete_session(pool: &PgPool, session_id_hash: &str) -> Result<()> {
    sqlx::query("DELETE FROM sessions WHERE id = $1")
        .bind(session_id_hash)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn prune_expired_sessions(pool: &PgPool) -> Result<u64> {
    let res = sqlx::query("DELETE FROM sessions WHERE expires_at <= NOW()")
        .execute(pool)
        .await?;
    Ok(res.rows_affected())
}

// ---------------------------------------------------------------------------
// WaniKani (scoped by user_id, api_token encrypted at rest)
// ---------------------------------------------------------------------------

pub async fn load_known_kanji(pool: &PgPool, user_id: i64) -> Result<HashSet<char>> {
    let rows: Vec<String> = sqlx::query_scalar(
        "SELECT characters FROM wk_subjects WHERE user_id = $1 AND subject_type = 'kanji'",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    let mut set = HashSet::new();
    for chars in rows {
        for c in chars.chars() {
            set.insert(c);
        }
    }
    Ok(set)
}

#[derive(FromRow)]
struct WkProfileRow {
    api_token: Option<String>,
    is_demo: bool,
    username: Option<String>,
    level: i32,
    lessons_available: i32,
    reviews_available: i32,
    smart_furigana: bool,
    highlight_known_kanji: bool,
    last_synced_at: Option<DateTime<Utc>>,
    sync_status: String,
    sync_progress_pages: i32,
    sync_progress_items: i32,
    sync_message: String,
    sync_error: Option<String>,
    auto_sync_enabled: bool,
    auto_sync_interval_mins: i32,
}

pub async fn load_wanikani_status(pool: &PgPool, user_id: i64) -> Result<WaniKaniStatus> {
    let profile: Option<WkProfileRow> = sqlx::query_as(
        "SELECT api_token, is_demo, username, level, lessons_available, reviews_available,
                smart_furigana, highlight_known_kanji, last_synced_at,
                sync_status, sync_progress_pages, sync_progress_items, sync_message, sync_error,
                auto_sync_enabled, auto_sync_interval_mins
         FROM wk_profile
         WHERE user_id = $1",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;

    let Some(profile) = profile else {
        return Ok(WaniKaniStatus {
            connected: false,
            is_demo: false,
            username: None,
            level: 0,
            kanji_known: 0,
            vocab_known: 0,
            wk_lessons_available: 0,
            wk_reviews_available: 0,
            smart_furigana: true,
            highlight_known_kanji: true,
            last_synced_at: None,
            known_kanji_list: Vec::new(),
            sync_status: "idle".to_string(),
            sync_progress_pages: 0,
            sync_progress_items: 0,
            sync_message: String::new(),
            sync_error: None,
            auto_sync_enabled: true,
            auto_sync_interval_mins: 60,
        });
    };

    let kanji_known: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM wk_subjects WHERE user_id = $1 AND subject_type = 'kanji'",
    )
    .bind(user_id)
    .fetch_one(pool)
    .await?;

    let vocab_known: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM wk_subjects WHERE user_id = $1 AND subject_type = 'vocabulary'",
    )
    .bind(user_id)
    .fetch_one(pool)
    .await?;

    let known_kanji_list: Vec<String> = sqlx::query_scalar(
        "SELECT characters
         FROM wk_subjects
         WHERE user_id = $1 AND subject_type = 'kanji'
         ORDER BY level, subject_id",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    let connected = profile.api_token.is_some() || profile.is_demo;

    Ok(WaniKaniStatus {
        connected,
        is_demo: profile.is_demo,
        username: profile.username,
        level: profile.level,
        kanji_known: kanji_known as usize,
        vocab_known: vocab_known as usize,
        wk_lessons_available: profile.lessons_available,
        wk_reviews_available: profile.reviews_available,
        smart_furigana: profile.smart_furigana,
        highlight_known_kanji: profile.highlight_known_kanji,
        last_synced_at: profile.last_synced_at.map(|dt| dt.to_rfc3339()),
        known_kanji_list,
        sync_status: profile.sync_status,
        sync_progress_pages: profile.sync_progress_pages,
        sync_progress_items: profile.sync_progress_items,
        sync_message: profile.sync_message,
        sync_error: profile.sync_error,
        auto_sync_enabled: profile.auto_sync_enabled,
        auto_sync_interval_mins: profile.auto_sync_interval_mins,
    })
}

pub async fn load_wanikani_sync_status(pool: &PgPool, user_id: i64) -> Result<WaniKaniSyncStatus> {
    let status = load_wanikani_status(pool, user_id).await?;
    Ok(WaniKaniSyncStatus {
        sync_status: status.sync_status,
        sync_progress_pages: status.sync_progress_pages,
        sync_progress_items: status.sync_progress_items,
        sync_message: status.sync_message,
        sync_error: status.sync_error,
        last_synced_at: status.last_synced_at,
        kanji_known: status.kanji_known,
        vocab_known: status.vocab_known,
    })
}

#[derive(FromRow)]
struct WkCursorRow {
    user_etag: Option<String>,
    summary_etag: Option<String>,
    assignments_updated_after: Option<DateTime<Utc>>,
    assignments_etag: Option<String>,
    subjects_updated_after: Option<DateTime<Utc>>,
    subjects_etag: Option<String>,
    username: Option<String>,
    level: i32,
    lessons_available: i32,
    reviews_available: i32,
}

pub async fn load_wanikani_sync_cursor(pool: &PgPool, user_id: i64) -> Result<WaniKaniSyncCursor> {
    let row: Option<WkCursorRow> = sqlx::query_as(
        "SELECT user_etag, summary_etag, assignments_updated_after, assignments_etag,
                subjects_updated_after, subjects_etag, username, level,
                lessons_available, reviews_available
         FROM wk_profile
         WHERE user_id = $1",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;

    Ok(match row {
        Some(r) => WaniKaniSyncCursor {
            user_etag: r.user_etag,
            summary_etag: r.summary_etag,
            assignments_updated_after: r.assignments_updated_after,
            assignments_etag: r.assignments_etag,
            subjects_updated_after: r.subjects_updated_after,
            subjects_etag: r.subjects_etag,
            cached_username: r.username,
            cached_level: r.level,
            cached_lessons: r.lessons_available,
            cached_reviews: r.reviews_available,
        },
        None => WaniKaniSyncCursor::default(),
    })
}

pub async fn save_wanikani_token_and_start_sync(
    pool: &PgPool,
    cipher: &TokenCipher,
    user_id: i64,
    api_token: &str,
    username: &str,
    level: i32,
    user_etag: Option<&str>,
) -> Result<WaniKaniStatus> {
    let encrypted = cipher.encrypt_for_user(user_id, api_token)?;
    sqlx::query(
        "INSERT INTO wk_profile
             (user_id, api_token, is_demo, username, level, user_etag,
              sync_status, sync_progress_pages, sync_progress_items, sync_message, sync_error, sync_started_at,
              assignments_updated_after, assignments_etag, subjects_updated_after, subjects_etag)
         VALUES ($1, $2, FALSE, $3, $4, $5, 'running', 1, 0, 'Queued full WaniKani sync...', NULL, NOW(), NULL, NULL, NULL, NULL)
         ON CONFLICT (user_id) DO UPDATE SET
             api_token = EXCLUDED.api_token,
             is_demo = FALSE,
             username = EXCLUDED.username,
             level = EXCLUDED.level,
             user_etag = EXCLUDED.user_etag,
             sync_status = 'running',
             sync_progress_pages = 1,
             sync_progress_items = 0,
             sync_message = 'Queued full WaniKani sync...',
             sync_error = NULL,
             sync_started_at = NOW(),
             assignments_updated_after = NULL,
             assignments_etag = NULL,
             subjects_updated_after = NULL,
             subjects_etag = NULL",
    )
    .bind(user_id)
    .bind(&encrypted)
    .bind(username)
    .bind(level)
    .bind(user_etag)
    .execute(pool)
    .await?;

    load_wanikani_status(pool, user_id).await
}

pub async fn mark_wanikani_sync_progress(
    pool: &PgPool,
    user_id: i64,
    status: &str,
    pages: i32,
    items: i32,
    message: &str,
    error: Option<&str>,
) -> Result<()> {
    sqlx::query(
        "UPDATE wk_profile
         SET sync_status = $2,
             sync_progress_pages = $3,
             sync_progress_items = $4,
             sync_message = $5,
             sync_error = $6,
             sync_started_at = CASE WHEN $2 = 'running' AND sync_started_at IS NULL THEN NOW() ELSE sync_started_at END
         WHERE user_id = $1",
    )
    .bind(user_id)
    .bind(status)
    .bind(pages)
    .bind(items)
    .bind(message)
    .bind(error)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_users_due_for_wk_auto_sync(pool: &PgPool) -> Result<Vec<i64>> {
    let ids: Vec<i64> = sqlx::query_scalar(
        "SELECT user_id
         FROM wk_profile
         WHERE api_token IS NOT NULL
           AND is_demo = FALSE
           AND auto_sync_enabled = TRUE
           AND sync_status <> 'running'
           AND (
               last_synced_at IS NULL
               OR last_synced_at <= NOW() - make_interval(mins => GREATEST(auto_sync_interval_mins, 15))
           )
         ORDER BY last_synced_at ASC NULLS FIRST
         LIMIT 20",
    )
    .fetch_all(pool)
    .await?;
    Ok(ids)
}

/// Spawns an asynchronous background WaniKani sync task for `user_id` with per-user deduplication.
pub async fn spawn_wanikani_sync_job(state: AppState, user_id: i64, force_full: bool) -> Result<bool> {
    {
        let mut inflight = state.wk_sync_inflight.lock().await;
        if !inflight.insert(user_id) {
            return Ok(false);
        }
    }

    let (token_opt, is_demo) = match get_wanikani_credentials(&state.pool, &state.cipher, user_id).await {
        Ok(v) => v,
        Err(e) => {
            state.wk_sync_inflight.lock().await.remove(&user_id);
            return Err(e);
        }
    };

    if is_demo {
        state.wk_sync_inflight.lock().await.remove(&user_id);
        return Ok(false);
    }

    let Some(api_token) = token_opt else {
        state.wk_sync_inflight.lock().await.remove(&user_id);
        return Ok(false);
    };

    let _ = mark_wanikani_sync_progress(
        &state.pool,
        user_id,
        "running",
        0,
        0,
        if force_full {
            "Starting full WaniKani sync..."
        } else {
            "Starting incremental WaniKani sync..."
        },
        None,
    )
    .await;

    tokio::spawn(async move {
        let result: Result<()> = async {
            let cursor = load_wanikani_sync_cursor(&state.pool, user_id).await?;
            let client = WaniKaniClient::new(state.wk_api_base_url.clone(), 500)?;
            let pool_for_progress = state.pool.clone();

            let sync_result = client
                .sync_with_progress(&api_token, &cursor, force_full, move |prog| {
                    let p = pool_for_progress.clone();
                    tokio::spawn(async move {
                        let _ = mark_wanikani_sync_progress(
                            &p,
                            user_id,
                            "running",
                            prog.pages_fetched,
                            prog.items_fetched,
                            &prog.message,
                            None,
                        )
                        .await;
                    });
                })
                .await?;

            save_wanikani_sync(
                &state.pool,
                &state.cipher,
                user_id,
                Some(&api_token),
                false,
                &sync_result,
            )
            .await?;
            Ok(())
        }
        .await;

        if let Err(err) = result {
            tracing::warn!(user_id, error = %err, "Background WaniKani sync failed");
            let _ = mark_wanikani_sync_progress(
                &state.pool,
                user_id,
                "failed",
                0,
                0,
                "WaniKani sync failed",
                Some(&err.to_string()),
            )
            .await;
        }

        state.wk_sync_inflight.lock().await.remove(&user_id);
    });

    Ok(true)
}

/// Spawns the periodic background worker that auto-syncs connected WaniKani users.
pub fn spawn_wanikani_auto_sync_worker(state: AppState) {
    let poll_secs: u64 = std::env::var("WK_AUTO_SYNC_POLL_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .map(|s: u64| s.clamp(30, 3600))
        .unwrap_or(300);

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(StdDuration::from_secs(poll_secs));
        interval.tick().await; // skip immediate tick at boot
        loop {
            interval.tick().await;
            match list_users_due_for_wk_auto_sync(&state.pool).await {
                Ok(user_ids) => {
                    for uid in user_ids {
                        if let Err(e) = spawn_wanikani_sync_job(state.clone(), uid, false).await {
                            tracing::warn!(user_id = uid, error = %e, "Auto-sync dispatch failed");
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to query users due for WaniKani auto-sync");
                }
            }
        }
    });
}

pub async fn get_wanikani_credentials(
    pool: &PgPool,
    cipher: &TokenCipher,
    user_id: i64,
) -> Result<(Option<String>, bool)> {
    let row: Option<(Option<String>, bool)> =
        sqlx::query_as("SELECT api_token, is_demo FROM wk_profile WHERE user_id = $1")
            .bind(user_id)
            .fetch_optional(pool)
            .await?;

    let Some((stored_token, is_demo)) = row else {
        return Ok((None, false));
    };

    let decrypted = match stored_token {
        Some(enc) => Some(cipher.decrypt_for_user(user_id, &enc)?),
        None => None,
    };

    Ok((decrypted, is_demo))
}

pub async fn save_wanikani_sync(
    pool: &PgPool,
    cipher: &TokenCipher,
    user_id: i64,
    api_token: Option<&str>,
    is_demo: bool,
    sync: &WaniKaniSyncResult,
) -> Result<WaniKaniStatus> {
    let encrypted_token = match api_token {
        Some(tok) => Some(cipher.encrypt_for_user(user_id, tok)?),
        None => None,
    };

    let now = Utc::now();
    let summary_msg = if sync.is_incremental {
        format!(
            "Incremental sync complete ({} updated, {} removed)",
            sync.subjects.len(),
            sync.removed_subject_ids.len()
        )
    } else {
        format!("Full sync complete ({} subjects)", sync.subjects.len())
    };
    let mut tx = pool.begin().await?;

    sqlx::query(
        "INSERT INTO wk_profile
             (user_id, api_token, is_demo, username, level, lessons_available, reviews_available,
              smart_furigana, highlight_known_kanji, last_synced_at,
              sync_status, sync_progress_items, sync_message, sync_error,
              user_etag, summary_etag, assignments_updated_after, assignments_etag,
              subjects_updated_after, subjects_etag)
         VALUES ($1, $2, $3, $4, $5, $6, $7, TRUE, TRUE, $8,
                 'completed', $9, $10, NULL, $11, $12, $13, $14, $15, $16)
         ON CONFLICT (user_id) DO UPDATE SET
             api_token = EXCLUDED.api_token,
             is_demo = EXCLUDED.is_demo,
             username = EXCLUDED.username,
             level = EXCLUDED.level,
             lessons_available = EXCLUDED.lessons_available,
             reviews_available = EXCLUDED.reviews_available,
             last_synced_at = EXCLUDED.last_synced_at,
             sync_status = 'completed',
             sync_progress_items = EXCLUDED.sync_progress_items,
             sync_message = EXCLUDED.sync_message,
             sync_error = NULL,
             user_etag = COALESCE(EXCLUDED.user_etag, wk_profile.user_etag),
             summary_etag = COALESCE(EXCLUDED.summary_etag, wk_profile.summary_etag),
             assignments_updated_after = COALESCE(EXCLUDED.assignments_updated_after, wk_profile.assignments_updated_after),
             assignments_etag = COALESCE(EXCLUDED.assignments_etag, wk_profile.assignments_etag),
             subjects_updated_after = COALESCE(EXCLUDED.subjects_updated_after, wk_profile.subjects_updated_after),
             subjects_etag = COALESCE(EXCLUDED.subjects_etag, wk_profile.subjects_etag)",
    )
    .bind(user_id)
    .bind(encrypted_token.as_deref())
    .bind(is_demo)
    .bind(&sync.username)
    .bind(sync.level)
    .bind(sync.lessons_available)
    .bind(sync.reviews_available)
    .bind(now)
    .bind(sync.subjects.len() as i32)
    .bind(&summary_msg)
    .bind(sync.user_etag.as_deref())
    .bind(sync.summary_etag.as_deref())
    .bind(sync.assignments_updated_after)
    .bind(sync.assignments_etag.as_deref())
    .bind(sync.subjects_updated_after)
    .bind(sync.subjects_etag.as_deref())
    .execute(&mut *tx)
    .await?;

    if !sync.is_incremental {
        sqlx::query("DELETE FROM wk_subjects WHERE user_id = $1")
            .bind(user_id)
            .execute(&mut *tx)
            .await?;
    } else if !sync.removed_subject_ids.is_empty() {
        sqlx::query("DELETE FROM wk_subjects WHERE user_id = $1 AND subject_id = ANY($2)")
            .bind(user_id)
            .bind(&sync.removed_subject_ids)
            .execute(&mut *tx)
            .await?;
    }

    if !sync.subjects.is_empty() {
        let mut ids = Vec::with_capacity(sync.subjects.len());
        let mut types = Vec::with_capacity(sync.subjects.len());
        let mut chars = Vec::with_capacity(sync.subjects.len());
        let mut levels = Vec::with_capacity(sync.subjects.len());
        let mut stages = Vec::with_capacity(sync.subjects.len());

        for (sid, stype, ch, lvl, srs) in &sync.subjects {
            ids.push(*sid);
            types.push(stype.as_str());
            chars.push(ch.as_str());
            levels.push(*lvl);
            stages.push(*srs);
        }

        sqlx::query(
            "INSERT INTO wk_subjects (user_id, subject_id, subject_type, characters, level, srs_stage)
             SELECT $1, u.subject_id, u.subject_type, u.characters, u.level, u.srs_stage
             FROM UNNEST($2::BIGINT[], $3::TEXT[], $4::TEXT[], $5::INTEGER[], $6::INTEGER[])
                 AS u(subject_id, subject_type, characters, level, srs_stage)
             ON CONFLICT (user_id, subject_id) DO UPDATE SET
                 subject_type = EXCLUDED.subject_type,
                 characters = EXCLUDED.characters,
                 level = EXCLUDED.level,
                 srs_stage = EXCLUDED.srs_stage",
        )
        .bind(user_id)
        .bind(&ids)
        .bind(&types)
        .bind(&chars)
        .bind(&levels)
        .bind(&stages)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    load_wanikani_status(pool, user_id).await
}

pub async fn disconnect_wanikani(pool: &PgPool, user_id: i64) -> Result<WaniKaniStatus> {
    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO wk_profile (user_id, api_token, is_demo, username, level, lessons_available, reviews_available, smart_furigana, highlight_known_kanji, last_synced_at)
         VALUES ($1, NULL, FALSE, NULL, 0, 0, 0, TRUE, TRUE, NULL)
         ON CONFLICT (user_id) DO UPDATE SET
             api_token = NULL,
             is_demo = FALSE,
             username = NULL,
             level = 0,
             lessons_available = 0,
             reviews_available = 0,
             last_synced_at = NULL,
             sync_status = 'idle',
             sync_progress_pages = 0,
             sync_progress_items = 0,
             sync_message = '',
             sync_error = NULL,
             user_etag = NULL,
             summary_etag = NULL,
             assignments_updated_after = NULL,
             assignments_etag = NULL,
             subjects_updated_after = NULL,
             subjects_etag = NULL",
    )
    .bind(user_id)
    .execute(&mut *tx)
    .await?;

    sqlx::query("DELETE FROM wk_subjects WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;
    load_wanikani_status(pool, user_id).await
}

pub async fn update_wanikani_settings(
    pool: &PgPool,
    user_id: i64,
    smart_furigana: bool,
    highlight_known_kanji: bool,
    auto_sync_enabled: Option<bool>,
    auto_sync_interval_mins: Option<i32>,
) -> Result<WaniKaniStatus> {
    let clamped_interval = auto_sync_interval_mins.map(|m| m.clamp(15, 1440));
    sqlx::query(
        "INSERT INTO wk_profile (user_id, smart_furigana, highlight_known_kanji, auto_sync_enabled, auto_sync_interval_mins)
         VALUES ($1, $2, $3, COALESCE($4, TRUE), COALESCE($5, 60))
         ON CONFLICT (user_id) DO UPDATE SET
             smart_furigana = EXCLUDED.smart_furigana,
             highlight_known_kanji = EXCLUDED.highlight_known_kanji,
             auto_sync_enabled = COALESCE($4, wk_profile.auto_sync_enabled),
             auto_sync_interval_mins = COALESCE($5, wk_profile.auto_sync_interval_mins)",
    )
    .bind(user_id)
    .bind(smart_furigana)
    .bind(highlight_known_kanji)
    .bind(auto_sync_enabled)
    .bind(clamped_interval)
    .execute(pool)
    .await?;

    load_wanikani_status(pool, user_id).await
}

// ---------------------------------------------------------------------------
// Grammar, Progress & Reviews (scoped by user_id)
// ---------------------------------------------------------------------------

#[derive(FromRow)]
struct GrammarSummaryRow {
    id: i64,
    slug: String,
    title: String,
    reading: String,
    meaning: String,
    jlpt_level: String,
    study_path: String,
    structure: String,
    srs_stage: i32,
    next_review_at: Option<DateTime<Utc>>,
    ghost_level: i32,
    correct_count: i32,
    incorrect_count: i32,
    combined_sentences: String,
}

pub async fn list_grammar_summaries(
    pool: &PgPool,
    user_id: i64,
    jlpt_filter: Option<&str>,
    query: Option<&str>,
    status_filter: Option<&str>,
    study_path_filter: Option<&str>,
) -> Result<Vec<GrammarPointSummary>> {
    let known_kanji = load_known_kanji(pool, user_id).await?;

    let rows: Vec<GrammarSummaryRow> = sqlx::query_as(
        "SELECT g.id, g.slug, g.title, g.reading, g.meaning, g.jlpt_level, g.study_path, g.structure,
                COALESCE(p.srs_stage, 0) AS srs_stage,
                p.next_review_at,
                COALESCE(p.ghost_level, 0) AS ghost_level,
                COALESCE(p.correct_count, 0) AS correct_count,
                COALESCE(p.incorrect_count, 0) AS incorrect_count,
                COALESCE(STRING_AGG(s.jp_before || s.answer || s.jp_after, ' ' ORDER BY s.position, s.id), '') AS combined_sentences
         FROM grammar_points g
         LEFT JOIN user_progress p ON p.grammar_id = g.id AND p.user_id = $1
         LEFT JOIN grammar_sentences s ON s.grammar_id = g.id
         GROUP BY g.id, p.srs_stage, p.next_review_at, p.ghost_level, p.correct_count, p.incorrect_count
         ORDER BY CASE g.jlpt_level WHEN 'N5' THEN 1 WHEN 'N4' THEN 2 WHEN 'N3' THEN 3 WHEN 'N2' THEN 4 ELSE 5 END, g.id",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    let q_lower = query.unwrap_or("").trim().to_lowercase();
    let jlpt = jlpt_filter.unwrap_or("ALL").to_uppercase();
    let status = status_filter.unwrap_or("all").to_lowercase();
    let path_filter = study_path_filter
        .unwrap_or("ALL")
        .trim()
        .to_lowercase();

    let mut items = Vec::new();
    for row in rows {
        if jlpt != "ALL" && row.jlpt_level != jlpt {
            continue;
        }
        if path_filter != "all"
            && path_filter != "jlpt core"
            && !path_filter.is_empty()
            && row.study_path.to_lowercase() != path_filter
        {
            continue;
        }

        let total_attempts = row.correct_count + row.incorrect_count;
        let is_leech = row.incorrect_count >= 2
            && total_attempts >= 3
            && (row.incorrect_count as f32 / total_attempts as f32) >= 0.35;

        match status.as_str() {
            "learned" if row.srs_stage == 0 => continue,
            "new" if row.srs_stage > 0 => continue,
            "ghost" if row.ghost_level == 0 => continue,
            "leech" if !is_leech => continue,
            _ => {}
        }

        if !q_lower.is_empty() {
            let hay = format!(
                "{} {} {} {} {}",
                row.title, row.reading, row.meaning, row.slug, row.study_path
            )
            .to_lowercase();
            if !hay.contains(&q_lower) {
                continue;
            }
        }

        let (wk_known, wk_total, wk_pct, _) =
            compute_kanji_readiness(&row.combined_sentences, &known_kanji);

        items.push(GrammarPointSummary {
            id: row.id,
            slug: row.slug,
            title: row.title,
            reading: row.reading,
            meaning: row.meaning,
            jlpt_level: row.jlpt_level,
            study_path: row.study_path,
            structure: row.structure,
            srs_stage: row.srs_stage,
            srs_label: srs_stage_label(row.srs_stage),
            next_review_at: row.next_review_at.map(|dt| dt.to_rfc3339()),
            ghost_level: row.ghost_level,
            is_leech,
            wk_readiness_pct: wk_pct,
            wk_known_kanji: wk_known,
            wk_total_kanji: wk_total,
        });
    }

    Ok(items)
}

#[derive(FromRow)]
struct GrammarDetailRow {
    id: i64,
    slug: String,
    title: String,
    reading: String,
    meaning: String,
    jlpt_level: String,
    study_path: String,
    structure: String,
    nuance: String,
    caution: String,
    confusable_with: String,
    pitch_note: String,
    user_notes: String,
    srs_stage: i32,
    correct_count: i32,
    incorrect_count: i32,
    streak: i32,
    ghost_level: i32,
    next_review_at: Option<DateTime<Utc>>,
}

#[derive(FromRow, Clone)]
struct GrammarSentenceRow {
    id: i64,
    grammar_id: i64,
    jp_before: String,
    jp_after: String,
    answer: String,
    alt_answers: Vec<String>,
    hint: String,
    english: String,
    nuance_note: String,
}

pub async fn get_grammar_detail(
    pool: &PgPool,
    user_id: i64,
    grammar_id: i64,
) -> Result<Option<GrammarPointDetail>> {
    let known_kanji = load_known_kanji(pool, user_id).await?;

    let row: Option<GrammarDetailRow> = sqlx::query_as(
        "SELECT g.id, g.slug, g.title, g.reading, g.meaning, g.jlpt_level, g.study_path,
                g.structure, g.nuance, g.caution, g.confusable_with, g.pitch_note,
                COALESCE(p.user_notes, '') AS user_notes,
                COALESCE(p.srs_stage, 0) AS srs_stage,
                COALESCE(p.correct_count, 0) AS correct_count,
                COALESCE(p.incorrect_count, 0) AS incorrect_count,
                COALESCE(p.streak, 0) AS streak,
                COALESCE(p.ghost_level, 0) AS ghost_level,
                p.next_review_at
         FROM grammar_points g
         LEFT JOIN user_progress p ON p.grammar_id = g.id AND p.user_id = $1
         WHERE g.id = $2",
    )
    .bind(user_id)
    .bind(grammar_id)
    .fetch_optional(pool)
    .await?;

    let Some(row) = row else {
        return Ok(None);
    };

    let sentence_rows: Vec<GrammarSentenceRow> = sqlx::query_as(
        "SELECT id, grammar_id, jp_before, jp_after, answer, alt_answers, hint, english, nuance_note
         FROM grammar_sentences
         WHERE grammar_id = $1
         ORDER BY position, id",
    )
    .bind(row.id)
    .fetch_all(pool)
    .await?;

    let sentences: Vec<GrammarSentence> = sentence_rows
        .into_iter()
        .map(|s| GrammarSentence {
            id: s.id,
            grammar_id: s.grammar_id,
            jp_before: s.jp_before,
            jp_after: s.jp_after,
            answer: s.answer,
            alt_answers: s.alt_answers,
            hint: s.hint,
            english: s.english,
            nuance_note: s.nuance_note,
        })
        .collect();

    let combined: String = sentences
        .iter()
        .map(|s| format!("{}{}{}", s.jp_before, s.answer, s.jp_after))
        .collect::<Vec<_>>()
        .join(" ");
    let (wk_known, wk_total, wk_pct, _) = compute_kanji_readiness(&combined, &known_kanji);

    Ok(Some(GrammarPointDetail {
        id: row.id,
        slug: row.slug,
        title: row.title,
        reading: row.reading,
        meaning: row.meaning,
        jlpt_level: row.jlpt_level,
        study_path: row.study_path,
        structure: row.structure,
        nuance: row.nuance,
        caution: row.caution,
        confusable_with: row.confusable_with,
        pitch_note: row.pitch_note,
        user_notes: row.user_notes,
        srs_stage: row.srs_stage,
        srs_label: srs_stage_label(row.srs_stage),
        correct_count: row.correct_count,
        incorrect_count: row.incorrect_count,
        streak: row.streak,
        ghost_level: row.ghost_level,
        next_review_at: row.next_review_at.map(|dt| dt.to_rfc3339()),
        sentences,
        wk_readiness_pct: wk_pct,
        wk_known_kanji: wk_known,
        wk_total_kanji: wk_total,
    }))
}

pub async fn update_grammar_notes(
    pool: &PgPool,
    user_id: i64,
    grammar_id: i64,
    notes: &str,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO user_progress (user_id, grammar_id, user_notes)
         VALUES ($1, $2, $3)
         ON CONFLICT (user_id, grammar_id) DO UPDATE SET
             user_notes = EXCLUDED.user_notes",
    )
    .bind(user_id)
    .bind(grammar_id)
    .bind(notes)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn reset_grammar_progress(
    pool: &PgPool,
    user_id: i64,
    grammar_id: i64,
) -> Result<()> {
    sqlx::query(
        "UPDATE user_progress
         SET srs_stage = 0,
             next_review_at = NULL,
             ghost_level = 0,
             ghost_due_at = NULL,
             streak = 0
         WHERE user_id = $1 AND grammar_id = $2",
    )
    .bind(user_id)
    .bind(grammar_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn start_lesson(
    pool: &PgPool,
    user_id: i64,
    grammar_id: i64,
    next_due: DateTime<Utc>,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO user_progress (user_id, grammar_id, srs_stage, next_review_at)
         VALUES ($1, $2, 1, $3)
         ON CONFLICT (user_id, grammar_id) DO UPDATE SET
             srs_stage = CASE WHEN user_progress.srs_stage = 0 THEN 1 ELSE user_progress.srs_stage END,
             next_review_at = COALESCE(user_progress.next_review_at, EXCLUDED.next_review_at)",
    )
    .bind(user_id)
    .bind(grammar_id)
    .bind(next_due)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_sentence_answers(
    pool: &PgPool,
    sentence_id: i64,
) -> Result<Option<(String, Vec<String>, String)>> {
    let row: Option<(String, Vec<String>, String)> =
        sqlx::query_as("SELECT answer, alt_answers, hint FROM grammar_sentences WHERE id = $1")
            .bind(sentence_id)
            .fetch_optional(pool)
            .await?;
    Ok(row)
}

pub async fn get_user_progress_stages(
    pool: &PgPool,
    user_id: i64,
    grammar_id: i64,
) -> Result<Option<(i32, i32)>> {
    let row: Option<(i32, i32)> = sqlx::query_as(
        "SELECT COALESCE(p.srs_stage, 0), COALESCE(p.ghost_level, 0)
         FROM grammar_points g
         LEFT JOIN user_progress p ON p.grammar_id = g.id AND p.user_id = $1
         WHERE g.id = $2",
    )
    .bind(user_id)
    .bind(grammar_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

#[allow(clippy::too_many_arguments)]
pub async fn record_review_submission(
    pool: &PgPool,
    user_id: i64,
    grammar_id: i64,
    mode: &str,
    is_ghost: bool,
    correct: bool,
    transition: &SrsTransition,
    now: DateTime<Utc>,
) -> Result<i64> {
    let mut tx = pool.begin().await?;

    let correct_delta: i32 = if correct { 1 } else { 0 };
    let incorrect_delta: i32 = if correct { 0 } else { 1 };
    let initial_streak: i32 = if correct { 1 } else { 0 };

    if is_ghost {
        sqlx::query(
            "INSERT INTO user_progress
                 (user_id, grammar_id, ghost_level, ghost_due_at, correct_count, incorrect_count)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (user_id, grammar_id) DO UPDATE SET
                 ghost_level = EXCLUDED.ghost_level,
                 ghost_due_at = EXCLUDED.ghost_due_at,
                 correct_count = user_progress.correct_count + EXCLUDED.correct_count,
                 incorrect_count = user_progress.incorrect_count + EXCLUDED.incorrect_count",
        )
        .bind(user_id)
        .bind(grammar_id)
        .bind(transition.new_ghost_level)
        .bind(transition.ghost_due_at)
        .bind(correct_delta)
        .bind(incorrect_delta)
        .execute(&mut *tx)
        .await?;
    } else {
        sqlx::query(
            "INSERT INTO user_progress
                 (user_id, grammar_id, srs_stage, next_review_at, ghost_level, ghost_due_at, correct_count, incorrect_count, streak)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
             ON CONFLICT (user_id, grammar_id) DO UPDATE SET
                 srs_stage = EXCLUDED.srs_stage,
                 next_review_at = EXCLUDED.next_review_at,
                 ghost_level = EXCLUDED.ghost_level,
                 ghost_due_at = COALESCE(EXCLUDED.ghost_due_at, user_progress.ghost_due_at),
                 correct_count = user_progress.correct_count + EXCLUDED.correct_count,
                 incorrect_count = user_progress.incorrect_count + EXCLUDED.incorrect_count,
                 streak = CASE WHEN $10 THEN user_progress.streak + 1 ELSE 0 END",
        )
        .bind(user_id)
        .bind(grammar_id)
        .bind(transition.new_stage)
        .bind(transition.next_review_at)
        .bind(transition.new_ghost_level)
        .bind(transition.ghost_due_at)
        .bind(correct_delta)
        .bind(incorrect_delta)
        .bind(initial_streak)
        .bind(correct)
        .execute(&mut *tx)
        .await?;
    }

    let log_id: i64 = sqlx::query_scalar(
        "INSERT INTO review_logs (user_id, grammar_id, correct, mode, is_ghost, reviewed_at)
         VALUES ($1, $2, $3, $4, $5, $6)
         RETURNING id",
    )
    .bind(user_id)
    .bind(grammar_id)
    .bind(correct)
    .bind(mode)
    .bind(is_ghost)
    .bind(now)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(log_id)
}

pub async fn undo_review(
    pool: &PgPool,
    user_id: i64,
    grammar_id: i64,
    previous_srs_stage: i32,
    previous_ghost_level: i32,
    review_log_id: Option<i64>,
    now: DateTime<Utc>,
) -> Result<()> {
    let restore_due = if previous_srs_stage > 0 {
        Some(now - Duration::minutes(1))
    } else {
        None
    };
    let restore_ghost_due = if previous_ghost_level > 0 {
        Some(now - Duration::minutes(1))
    } else {
        None
    };

    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO user_progress (user_id, grammar_id, srs_stage, next_review_at, ghost_level, ghost_due_at)
         VALUES ($1, $2, $3, $4, $5, $6)
         ON CONFLICT (user_id, grammar_id) DO UPDATE SET
             srs_stage = EXCLUDED.srs_stage,
             next_review_at = EXCLUDED.next_review_at,
             ghost_level = EXCLUDED.ghost_level,
             ghost_due_at = EXCLUDED.ghost_due_at",
    )
    .bind(user_id)
    .bind(grammar_id)
    .bind(previous_srs_stage)
    .bind(restore_due)
    .bind(previous_ghost_level)
    .bind(restore_ghost_due)
    .execute(&mut *tx)
    .await?;

    if let Some(lid) = review_log_id {
        sqlx::query("DELETE FROM review_logs WHERE id = $1 AND user_id = $2")
            .bind(lid)
            .bind(user_id)
            .execute(&mut *tx)
            .await?;
    }

    tx.commit().await?;
    Ok(())
}

#[derive(FromRow)]
struct QueueGrammarRow {
    id: i64,
    title: String,
    reading: String,
    meaning: String,
    jlpt_level: String,
    structure: String,
    nuance: String,
    caution: String,
    confusable_with: String,
    srs_stage: i32,
    ghost_level: i32,
}

pub async fn build_review_queue(
    pool: &PgPool,
    user_id: i64,
    mode: &str,
    jlpt_filter: Option<&str>,
    limit: usize,
) -> Result<Vec<ReviewQueueItem>> {
    let now = Utc::now();
    let jlpt = jlpt_filter.unwrap_or("ALL").to_uppercase();
    let settings = get_user_srs_settings(pool, user_id).await?;

    // Vacation mode pauses due reviews & ghost reviews (Cram & Lessons stay available)
    if settings.vacation_mode && (mode == "review" || mode == "ghost") {
        return Ok(Vec::new());
    }

    let mut candidates: Vec<(QueueGrammarRow, bool, &'static str)> = Vec::new();
    let mut seen_grammar: HashSet<i64> = HashSet::new();

    let mut accept =
        |rows: Vec<QueueGrammarRow>, is_ghost_fn: fn(i32) -> bool, item_mode: &'static str| -> bool {
            for row in rows {
                if candidates.len() >= limit {
                    break;
                }
                if jlpt != "ALL" && row.jlpt_level != jlpt {
                    continue;
                }
                if !seen_grammar.insert(row.id) {
                    continue;
                }
                let is_ghost = is_ghost_fn(row.ghost_level);
                candidates.push((row, is_ghost, item_mode));
            }
            candidates.len() < limit
        };

    match mode {
        "ghost" => {
            let rows: Vec<QueueGrammarRow> = sqlx::query_as(
                "SELECT g.id, g.title, g.reading, g.meaning, g.jlpt_level, g.structure, g.nuance, g.caution, g.confusable_with,
                        p.srs_stage, p.ghost_level
                 FROM grammar_points g
                 JOIN user_progress p ON p.grammar_id = g.id AND p.user_id = $1
                 WHERE p.ghost_level > 0
                 ORDER BY p.ghost_due_at ASC NULLS LAST, g.id",
            )
            .bind(user_id)
            .fetch_all(pool)
            .await?;
            accept(rows, |_| true, "review");
        }
        "lesson" => {
            let rows: Vec<QueueGrammarRow> = sqlx::query_as(
                "SELECT g.id, g.title, g.reading, g.meaning, g.jlpt_level, g.structure, g.nuance, g.caution, g.confusable_with,
                        COALESCE(p.srs_stage, 0) AS srs_stage,
                        COALESCE(p.ghost_level, 0) AS ghost_level
                 FROM grammar_points g
                 LEFT JOIN user_progress p ON p.grammar_id = g.id AND p.user_id = $1
                 WHERE COALESCE(p.srs_stage, 0) = 0
                 ORDER BY CASE WHEN $2 <> 'JLPT Core' AND g.study_path = $2 THEN 0 ELSE 1 END,
                          CASE g.jlpt_level WHEN 'N5' THEN 1 WHEN 'N4' THEN 2 WHEN 'N3' THEN 3 WHEN 'N2' THEN 4 ELSE 5 END,
                          g.id",
            )
            .bind(user_id)
            .bind(&settings.preferred_study_path)
            .fetch_all(pool)
            .await?;
            accept(rows, |_| false, "lesson");
        }
        "cram" => {
            let rows: Vec<QueueGrammarRow> = sqlx::query_as(
                "SELECT g.id, g.title, g.reading, g.meaning, g.jlpt_level, g.structure, g.nuance, g.caution, g.confusable_with,
                        COALESCE(p.srs_stage, 0) AS srs_stage,
                        COALESCE(p.ghost_level, 0) AS ghost_level
                 FROM grammar_points g
                 LEFT JOIN user_progress p ON p.grammar_id = g.id AND p.user_id = $1
                 ORDER BY COALESCE(p.ghost_level, 0) DESC, COALESCE(p.incorrect_count, 0) DESC, COALESCE(p.srs_stage, 0) DESC, g.id",
            )
            .bind(user_id)
            .fetch_all(pool)
            .await?;
            accept(rows, |gl| gl > 0, "cram");
        }
        _ => {
            // Due Ghost Reviews first, then due regular SRS reviews
            let ghost_rows: Vec<QueueGrammarRow> = sqlx::query_as(
                "SELECT g.id, g.title, g.reading, g.meaning, g.jlpt_level, g.structure, g.nuance, g.caution, g.confusable_with,
                        p.srs_stage, p.ghost_level
                 FROM grammar_points g
                 JOIN user_progress p ON p.grammar_id = g.id AND p.user_id = $1
                 WHERE p.ghost_level > 0 AND p.ghost_due_at IS NOT NULL AND p.ghost_due_at <= $2
                 ORDER BY p.ghost_due_at ASC, g.id",
            )
            .bind(user_id)
            .bind(now)
            .fetch_all(pool)
            .await?;
            if accept(ghost_rows, |_| true, "review") {
                let regular_rows: Vec<QueueGrammarRow> = sqlx::query_as(
                    "SELECT g.id, g.title, g.reading, g.meaning, g.jlpt_level, g.structure, g.nuance, g.caution, g.confusable_with,
                            p.srs_stage, p.ghost_level
                     FROM grammar_points g
                     JOIN user_progress p ON p.grammar_id = g.id AND p.user_id = $1
                     WHERE p.srs_stage > 0 AND p.srs_stage < 12 AND p.next_review_at IS NOT NULL AND p.next_review_at <= $2
                     ORDER BY p.next_review_at ASC, g.id",
                )
                .bind(user_id)
                .bind(now)
                .fetch_all(pool)
                .await?;
                accept(regular_rows, |_| false, "review");
            }
        }
    }

    if candidates.is_empty() {
        return Ok(Vec::new());
    }

    let grammar_ids: Vec<i64> = candidates.iter().map(|(g, _, _)| g.id).collect();
    let sentence_rows: Vec<GrammarSentenceRow> = sqlx::query_as(
        "SELECT id, grammar_id, jp_before, jp_after, answer, alt_answers, hint, english, nuance_note
         FROM grammar_sentences
         WHERE grammar_id = ANY($1)
         ORDER BY grammar_id, position, id",
    )
    .bind(&grammar_ids)
    .fetch_all(pool)
    .await?;

    let mut sentences_by_grammar: HashMap<i64, Vec<GrammarSentenceRow>> = HashMap::new();
    for s in sentence_rows {
        sentences_by_grammar
            .entry(s.grammar_id)
            .or_default()
            .push(s);
    }

    let mut items = Vec::with_capacity(candidates.len());
    for (g, is_ghost, item_mode) in candidates {
        let Some(sentences) = sentences_by_grammar.get(&g.id) else {
            continue;
        };
        if sentences.is_empty() {
            continue;
        }
        // Rotate sentence by SRS stage so higher reviews test alternate example sentences
        let idx = (g.srs_stage.max(0) as usize) % sentences.len();
        let s = &sentences[idx];

        items.push(ReviewQueueItem {
            grammar_id: g.id,
            sentence_id: s.id,
            title: g.title,
            reading: g.reading,
            meaning: g.meaning,
            jlpt_level: g.jlpt_level,
            structure: g.structure,
            nuance: g.nuance,
            caution: g.caution,
            confusable_with: g.confusable_with,
            jp_before: s.jp_before.clone(),
            jp_after: s.jp_after.clone(),
            answer: s.answer.clone(),
            alt_answers: s.alt_answers.clone(),
            hint: s.hint.clone(),
            english: s.english.clone(),
            nuance_note: s.nuance_note.clone(),
            srs_stage: g.srs_stage,
            srs_label: srs_stage_label(g.srs_stage),
            is_ghost,
            ghost_level: g.ghost_level,
            mode: item_mode.to_string(),
        });
    }

    Ok(items)
}

#[derive(FromRow)]
struct GrammarMatchRow {
    id: i64,
    title: String,
    meaning: String,
    jlpt_level: String,
    match_patterns: Vec<String>,
    srs_stage: i32,
}

async fn load_grammar_match_rows(pool: &PgPool, user_id: i64) -> Result<Vec<GrammarMatchRow>> {
    let rows: Vec<GrammarMatchRow> = sqlx::query_as(
        "SELECT g.id, g.title, g.meaning, g.jlpt_level, g.match_patterns,
                COALESCE(p.srs_stage, 0) AS srs_stage
         FROM grammar_points g
         LEFT JOIN user_progress p ON p.grammar_id = g.id AND p.user_id = $1
         ORDER BY g.id",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

#[derive(FromRow)]
struct WkSubjectLookupRow {
    subject_type: String,
    characters: String,
    level: i32,
}

pub async fn load_wk_subject_lookup(pool: &PgPool, user_id: i64) -> Result<WkSubjectLookup> {
    let rows: Vec<WkSubjectLookupRow> = sqlx::query_as(
        "SELECT subject_type, characters, level
         FROM wk_subjects
         WHERE user_id = $1",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    let mut lookup = WkSubjectLookup::default();
    for row in rows {
        if row.subject_type == "kanji" {
            lookup.known_kanji.insert(row.characters.clone());
            lookup.kanji_levels.insert(row.characters, row.level);
        } else if row.subject_type == "vocabulary" {
            lookup.vocab_levels.insert(row.characters, row.level);
        }
    }
    Ok(lookup)
}

fn match_grammar_patterns(
    rows: &[GrammarMatchRow],
    plain: &str,
    analysis: &NlpAnalysis,
) -> Vec<DetectedGrammar> {
    let mut found = Vec::new();
    for row in rows {
        if let Some(matched) = row
            .match_patterns
            .iter()
            .filter(|pat| !pat.is_empty())
            .find_map(|pat| match_pattern_against_analysis(pat.as_str(), plain, analysis))
        {
            found.push(DetectedGrammar {
                grammar_id: row.id,
                title: row.title.clone(),
                meaning: row.meaning.clone(),
                jlpt_level: row.jlpt_level.clone(),
                srs_stage: row.srs_stage,
                srs_label: srs_stage_label(row.srs_stage),
                matched_token: matched,
            });
        }
    }
    found
}

pub async fn detect_grammar_in_text(
    pool: &PgPool,
    user_id: i64,
    text: &str,
) -> Result<Vec<DetectedGrammar>> {
    let rows = load_grammar_match_rows(pool, user_id).await?;
    let wk = load_wk_subject_lookup(pool, user_id).await?;
    let plain = strip_ruby_markup(text);
    let analysis = analyze_japanese_text(&plain, &wk);
    Ok(match_grammar_patterns(&rows, &plain, &analysis))
}

fn known_kanji_chars(set: &HashSet<String>) -> HashSet<char> {
    set.iter().filter_map(|s| s.chars().next()).collect()
}

pub async fn analyze_reading_text(
    pool: &PgPool,
    user_id: i64,
    text: &str,
) -> Result<ReadingAnalyzeResponse> {
    let wk = load_wk_subject_lookup(pool, user_id).await?;
    let known_chars = known_kanji_chars(&wk.known_kanji);
    let grammar_rows = load_grammar_match_rows(pool, user_id).await?;
    let plain = strip_ruby_markup(text);
    let (known_cnt, total_cnt, pct, unknown) = compute_kanji_readiness(&plain, &known_chars);
    let analysis = analyze_japanese_text(&plain, &wk);
    let detected = match_grammar_patterns(&grammar_rows, &plain, &analysis);

    Ok(ReadingAnalyzeResponse {
        total_kanji: total_cnt,
        known_kanji: known_cnt,
        unknown_kanji: unknown,
        wk_readiness_pct: pct,
        detected_grammar: detected,
        tokens: analysis.tokens,
        annotated_ruby: analysis.annotated_ruby,
        analyzer_engine: analysis.analyzer_engine,
    })
}

pub async fn get_sentence_audio_info(
    pool: &PgPool,
    sentence_id: i64,
) -> Result<Option<(String, String)>> {
    let row: Option<(String, String, String, String)> = sqlx::query_as(
        "SELECT COALESCE(audio_url, ''), jp_before, answer, jp_after
         FROM grammar_sentences
         WHERE id = $1",
    )
    .bind(sentence_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|(audio_url, before, answer, after)| {
        let full = format!("{}{}{}", before, answer, after);
        (audio_url, strip_ruby_markup(&full))
    }))
}

#[derive(FromRow)]
struct ReadingPassageRow {
    id: i64,
    title: String,
    jlpt_level: String,
    topic: String,
    content_ruby: String,
    english: String,
}

pub async fn list_reading_passages(pool: &PgPool, user_id: i64) -> Result<Vec<ReadingPassage>> {
    let wk = load_wk_subject_lookup(pool, user_id).await?;
    let known_chars = known_kanji_chars(&wk.known_kanji);
    let grammar_rows = load_grammar_match_rows(pool, user_id).await?;

    let rows: Vec<ReadingPassageRow> = sqlx::query_as(
        "SELECT id, title, jlpt_level, topic, content_ruby, english
         FROM reading_passages
         ORDER BY id",
    )
    .fetch_all(pool)
    .await?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let plain = strip_ruby_markup(&row.content_ruby);
        let (_, _, wk_pct, _) = compute_kanji_readiness(&plain, &known_chars);
        let analysis = analyze_japanese_text(&plain, &wk);
        let detected = match_grammar_patterns(&grammar_rows, &plain, &analysis);
        out.push(ReadingPassage {
            id: row.id,
            title: row.title,
            jlpt_level: row.jlpt_level,
            topic: row.topic,
            content_ruby: row.content_ruby,
            english: row.english,
            wk_readiness_pct: wk_pct,
            detected_grammar: detected,
            tokens: analysis.tokens,
        });
    }
    Ok(out)
}

#[derive(FromRow)]
struct ConjugationRow {
    id: i64,
    dictionary: String,
    reading: String,
    meaning: String,
    word_type: String,
    target_form: String,
    target_label: String,
    answer_kana: String,
    answer_kanji: String,
    rule_explanation: String,
    example_jp: String,
    example_en: String,
}

pub async fn list_conjugations(
    pool: &PgPool,
    form: Option<&str>,
) -> Result<Vec<ConjugationItem>> {
    let form_filter = form.unwrap_or("all").to_lowercase();

    let rows: Vec<ConjugationRow> = if form_filter == "all" {
        sqlx::query_as(
            "SELECT id, dictionary, reading, meaning, word_type, target_form, target_label,
                    answer_kana, answer_kanji, rule_explanation, example_jp, example_en
             FROM conjugation_items
             ORDER BY id",
        )
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as(
            "SELECT id, dictionary, reading, meaning, word_type, target_form, target_label,
                    answer_kana, answer_kanji, rule_explanation, example_jp, example_en
             FROM conjugation_items
             WHERE target_form = $1
             ORDER BY id",
        )
        .bind(&form_filter)
        .fetch_all(pool)
        .await?
    };

    Ok(rows
        .into_iter()
        .map(|r| ConjugationItem {
            id: r.id,
            dictionary: r.dictionary,
            reading: r.reading,
            meaning: r.meaning,
            word_type: r.word_type,
            target_form: r.target_form,
            target_label: r.target_label,
            answer_kana: r.answer_kana,
            answer_kanji: r.answer_kanji,
            rule_explanation: r.rule_explanation,
            example_jp: r.example_jp,
            example_en: r.example_en,
        })
        .collect())
}

#[derive(FromRow)]
struct UserPrefsRow {
    onboarding_completed: bool,
    preferred_study_path: String,
    placement_jlpt: String,
    daily_goal: i32,
    vacation_mode: bool,
    vacation_started_at: Option<DateTime<Utc>>,
    ghost_intensity: String,
}

async fn load_user_prefs(pool: &PgPool, user_id: i64) -> Result<UserPrefsRow> {
    let row: UserPrefsRow = sqlx::query_as(
        "SELECT onboarding_completed, preferred_study_path, placement_jlpt, daily_goal,
                vacation_mode, vacation_started_at, ghost_intensity
         FROM users
         WHERE id = $1",
    )
    .bind(user_id)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

fn normalize_study_path(raw: &str) -> &'static str {
    match raw.trim().to_ascii_lowercase().as_str() {
        "genki i" | "genki 1" => "Genki I",
        "genki ii" | "genki 2" => "Genki II",
        "tobira" => "Tobira",
        "quartet" => "Quartet",
        _ => "JLPT Core",
    }
}

fn normalize_jlpt(raw: &str) -> &'static str {
    match raw.trim().to_ascii_uppercase().as_str() {
        "N4" => "N4",
        "N3" => "N3",
        "N2" => "N2",
        "N1" => "N1",
        _ => "N5",
    }
}

#[derive(FromRow)]
struct PlacementRow {
    grammar_id: i64,
    slug: String,
    title: String,
    meaning: String,
    jlpt_level: String,
    study_path: String,
    jp_before: String,
    answer: String,
    jp_after: String,
    english: String,
}

/// Returns a curated 10-question diagnostic placement check spanning N5 to N2.
pub async fn get_placement_questions(pool: &PgPool) -> Result<Vec<PlacementQuestion>> {
    let slugs = [
        "te-iru",
        "nakereba-naranai",
        "ta-koto-ga-aru",
        "noni",
        "te-oku",
        "tame-ni",
        "okage-de",
        "wake-dewa-nai",
        "ni-mo-kakawarazu",
        "wake-niwa-ikanai",
    ];

    let rows: Vec<PlacementRow> = sqlx::query_as(
        "SELECT g.id AS grammar_id, g.slug, g.title, g.meaning, g.jlpt_level, g.study_path,
                s.jp_before, s.answer, s.jp_after, s.english
         FROM grammar_points g
         JOIN grammar_sentences s ON s.grammar_id = g.id AND s.position = 0
         WHERE g.slug = ANY($1)
         ORDER BY CASE g.jlpt_level WHEN 'N5' THEN 1 WHEN 'N4' THEN 2 WHEN 'N3' THEN 3 WHEN 'N2' THEN 4 ELSE 5 END, g.id",
    )
    .bind(&slugs[..])
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| PlacementQuestion {
            grammar_id: r.grammar_id,
            slug: r.slug,
            title: r.title,
            meaning: r.meaning,
            jlpt_level: r.jlpt_level,
            study_path: r.study_path,
            sample_jp: strip_ruby_markup(&format!("{}{}{}", r.jp_before, r.answer, r.jp_after)),
            sample_en: r.english,
        })
        .collect())
}

pub async fn complete_onboarding(
    pool: &PgPool,
    user_id: i64,
    req: &CompleteOnboardingRequest,
) -> Result<OnboardingState> {
    let study_path = normalize_study_path(&req.study_path);
    let placement_jlpt = normalize_jlpt(&req.placement_jlpt);
    let daily_goal = req.daily_goal.unwrap_or(20).clamp(5, 200);
    let next_due = Utc::now() + Duration::hours(srs_interval_hours(5).unwrap_or(96));

    let mut tx = pool.begin().await?;
    sqlx::query(
        "UPDATE users
         SET onboarding_completed = TRUE,
             preferred_study_path = $2,
             placement_jlpt = $3,
             daily_goal = $4,
             updated_at = NOW()
         WHERE id = $1",
    )
    .bind(user_id)
    .bind(study_path)
    .bind(placement_jlpt)
    .bind(daily_goal)
    .execute(&mut *tx)
    .await?;

    // Mark self-assessed placement items at Guru I (stage 5) without fabricating review logs
    for &gid in &req.known_grammar_ids {
        sqlx::query(
            "INSERT INTO user_progress (user_id, grammar_id, srs_stage, next_review_at)
             SELECT $1, id, 5, $3 FROM grammar_points WHERE id = $2
             ON CONFLICT (user_id, grammar_id) DO UPDATE SET
                 srs_stage = CASE WHEN user_progress.srs_stage = 0 THEN 5 ELSE user_progress.srs_stage END,
                 next_review_at = COALESCE(user_progress.next_review_at, EXCLUDED.next_review_at)",
        )
        .bind(user_id)
        .bind(gid)
        .bind(next_due)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;

    Ok(OnboardingState {
        completed: true,
        preferred_study_path: study_path.to_string(),
        placement_jlpt: placement_jlpt.to_string(),
        daily_goal,
        content_version: CONTENT_SCHEMA_VERSION,
    })
}

pub async fn get_user_srs_settings(pool: &PgPool, user_id: i64) -> Result<SrsSettings> {
    let prefs = load_user_prefs(pool, user_id).await?;
    Ok(SrsSettings {
        vacation_mode: prefs.vacation_mode,
        vacation_started_at: prefs.vacation_started_at.map(|d| d.to_rfc3339()),
        ghost_intensity: GhostIntensity::parse(&prefs.ghost_intensity)
            .as_str()
            .to_string(),
        daily_goal: prefs.daily_goal,
        preferred_study_path: prefs.preferred_study_path,
    })
}

pub async fn update_srs_settings(
    pool: &PgPool,
    user_id: i64,
    req: &UpdateSrsSettingsRequest,
) -> Result<SrsSettings> {
    let mut tx = pool.begin().await?;
    let current: UserPrefsRow = sqlx::query_as(
        "SELECT onboarding_completed, preferred_study_path, placement_jlpt, daily_goal,
                vacation_mode, vacation_started_at, ghost_intensity
         FROM users
         WHERE id = $1
         FOR UPDATE",
    )
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await?;

    let now = Utc::now();
    let new_vacation = req.vacation_mode.unwrap_or(current.vacation_mode);
    let new_vacation_started_at = match (current.vacation_mode, new_vacation) {
        (false, true) => Some(now),
        (true, false) => {
            if let Some(started) = current.vacation_started_at {
                let paused_secs = (now - started).num_seconds().max(0);
                if paused_secs > 0 {
                    sqlx::query(
                        "UPDATE user_progress
                         SET next_review_at = CASE
                                 WHEN next_review_at IS NOT NULL THEN next_review_at + make_interval(secs => $2)
                                 ELSE NULL
                             END,
                             ghost_due_at = CASE
                                 WHEN ghost_due_at IS NOT NULL THEN ghost_due_at + make_interval(secs => $2)
                                 ELSE NULL
                             END
                         WHERE user_id = $1",
                    )
                    .bind(user_id)
                    .bind(paused_secs as f64)
                    .execute(&mut *tx)
                    .await?;
                }
            }
            None
        }
        (true, true) => current.vacation_started_at.or(Some(now)),
        (false, false) => None,
    };

    let new_intensity = req
        .ghost_intensity
        .as_deref()
        .map(|s| GhostIntensity::parse(s).as_str().to_string())
        .unwrap_or_else(|| GhostIntensity::parse(&current.ghost_intensity).as_str().to_string());
    let new_goal = req.daily_goal.unwrap_or(current.daily_goal).clamp(5, 200);
    let new_path = req
        .preferred_study_path
        .as_deref()
        .map(|s| normalize_study_path(s).to_string())
        .unwrap_or(current.preferred_study_path);

    sqlx::query(
        "UPDATE users
         SET vacation_mode = $2,
             vacation_started_at = $3,
             ghost_intensity = $4,
             daily_goal = $5,
             preferred_study_path = $6,
             updated_at = NOW()
         WHERE id = $1",
    )
    .bind(user_id)
    .bind(new_vacation)
    .bind(new_vacation_started_at)
    .bind(&new_intensity)
    .bind(new_goal)
    .bind(&new_path)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(SrsSettings {
        vacation_mode: new_vacation,
        vacation_started_at: new_vacation_started_at.map(|d| d.to_rfc3339()),
        ghost_intensity: new_intensity,
        daily_goal: new_goal,
        preferred_study_path: new_path,
    })
}

#[derive(FromRow)]
struct ExportDbRow {
    slug: String,
    title: String,
    jlpt_level: String,
    srs_stage: i32,
    ghost_level: i32,
    correct_count: i32,
    incorrect_count: i32,
    streak: i32,
    next_review_at: Option<DateTime<Utc>>,
    ghost_due_at: Option<DateTime<Utc>>,
    user_notes: String,
}

pub async fn export_user_progress(pool: &PgPool, user_id: i64) -> Result<ProgressExportPayload> {
    let settings = get_user_srs_settings(pool, user_id).await?;
    let rows: Vec<ExportDbRow> = sqlx::query_as(
        "SELECT g.slug, g.title, g.jlpt_level,
                p.srs_stage, p.ghost_level, p.correct_count, p.incorrect_count, p.streak,
                p.next_review_at, p.ghost_due_at, p.user_notes
         FROM user_progress p
         JOIN grammar_points g ON g.id = p.grammar_id
         WHERE p.user_id = $1
         ORDER BY CASE g.jlpt_level WHEN 'N5' THEN 1 WHEN 'N4' THEN 2 WHEN 'N3' THEN 3 WHEN 'N2' THEN 4 ELSE 5 END, g.id",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    let items = rows
        .into_iter()
        .map(|r| ProgressExportRow {
            slug: r.slug,
            title: r.title,
            jlpt_level: r.jlpt_level,
            srs_stage: r.srs_stage,
            ghost_level: r.ghost_level,
            correct_count: r.correct_count,
            incorrect_count: r.incorrect_count,
            streak: r.streak,
            next_review_at: r.next_review_at.map(|d| d.to_rfc3339()),
            ghost_due_at: r.ghost_due_at.map(|d| d.to_rfc3339()),
            user_notes: r.user_notes,
        })
        .collect();

    Ok(ProgressExportPayload {
        exported_at: Utc::now().to_rfc3339(),
        version: CONTENT_SCHEMA_VERSION,
        settings,
        items,
    })
}

fn csv_escape(field: &str) -> String {
    if field.contains(',') || field.contains('"') || field.contains('\n') || field.contains('\r') {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_string()
    }
}

pub fn progress_export_to_csv(payload: &ProgressExportPayload) -> String {
    let mut out = String::from(
        "slug,title,jlpt_level,srs_stage,ghost_level,correct_count,incorrect_count,streak,next_review_at,ghost_due_at,user_notes\n",
    );
    for row in &payload.items {
        let fields = [
            csv_escape(&row.slug),
            csv_escape(&row.title),
            csv_escape(&row.jlpt_level),
            row.srs_stage.to_string(),
            row.ghost_level.to_string(),
            row.correct_count.to_string(),
            row.incorrect_count.to_string(),
            row.streak.to_string(),
            csv_escape(row.next_review_at.as_deref().unwrap_or("")),
            csv_escape(row.ghost_due_at.as_deref().unwrap_or("")),
            csv_escape(&row.user_notes),
        ];
        out.push_str(&fields.join(","));
        out.push('\n');
    }
    out
}

fn parse_csv_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut chars = line.chars().peekable();
    let mut in_quotes = false;

    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    current.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                current.push(c);
            }
        } else if c == '"' {
            in_quotes = true;
        } else if c == ',' {
            fields.push(std::mem::take(&mut current));
        } else {
            current.push(c);
        }
    }
    fields.push(current);
    fields
}

#[derive(serde::Deserialize)]
struct ImportRowInput {
    slug: String,
    #[serde(default)]
    srs_stage: i32,
    #[serde(default)]
    ghost_level: i32,
    #[serde(default)]
    correct_count: i32,
    #[serde(default)]
    incorrect_count: i32,
    #[serde(default)]
    streak: i32,
    #[serde(default)]
    next_review_at: Option<String>,
    #[serde(default)]
    ghost_due_at: Option<String>,
    #[serde(default)]
    user_notes: String,
}

#[derive(serde::Deserialize)]
struct ImportJsonEnvelope {
    items: Vec<ImportRowInput>,
}

pub async fn import_user_progress(
    pool: &PgPool,
    user_id: i64,
    format: &str,
    data: &str,
) -> Result<ImportProgressResponse> {
    let mut rows_to_import: Vec<ImportRowInput> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    match format.trim().to_ascii_lowercase().as_str() {
        "json" => {
            if let Ok(env) = serde_json::from_str::<ImportJsonEnvelope>(data) {
                rows_to_import = env.items;
            } else if let Ok(list) = serde_json::from_str::<Vec<ImportRowInput>>(data) {
                rows_to_import = list;
            } else {
                anyhow::bail!("Invalid JSON progress payload (expected {{ items: [...] }} or array)");
            }
        }
        "csv" => {
            let mut lines = data.lines().filter(|l| !l.trim().is_empty());
            let Some(header_line) = lines.next() else {
                return Ok(ImportProgressResponse {
                    imported: 0,
                    skipped: 0,
                    errors: vec!["Empty CSV file".to_string()],
                });
            };
            let headers: Vec<String> = parse_csv_line(header_line)
                .into_iter()
                .map(|h| h.trim().to_ascii_lowercase())
                .collect();
            let col = |name: &str| headers.iter().position(|h| h == name);
            let Some(slug_idx) = col("slug") else {
                anyhow::bail!("CSV header must include a 'slug' column");
            };
            for (idx, line) in lines.enumerate() {
                let cols = parse_csv_line(line);
                let get = |name: &str| -> String {
                    col(name)
                        .and_then(|i| cols.get(i))
                        .map(|s| s.trim().to_string())
                        .unwrap_or_default()
                };
                let slug = cols
                    .get(slug_idx)
                    .map(|s| s.trim().to_string())
                    .unwrap_or_default();
                if slug.is_empty() {
                    errors.push(format!("Row {}: missing slug", idx + 2));
                    continue;
                }
                rows_to_import.push(ImportRowInput {
                    slug,
                    srs_stage: get("srs_stage").parse().unwrap_or(0),
                    ghost_level: get("ghost_level").parse().unwrap_or(0),
                    correct_count: get("correct_count").parse().unwrap_or(0),
                    incorrect_count: get("incorrect_count").parse().unwrap_or(0),
                    streak: get("streak").parse().unwrap_or(0),
                    next_review_at: {
                        let s = get("next_review_at");
                        if s.is_empty() { None } else { Some(s) }
                    },
                    ghost_due_at: {
                        let s = get("ghost_due_at");
                        if s.is_empty() { None } else { Some(s) }
                    },
                    user_notes: get("user_notes"),
                });
            }
        }
        other => anyhow::bail!("Unsupported import format '{}'. Use 'json' or 'csv'.", other),
    }

    let slug_rows: Vec<(String, i64)> =
        sqlx::query_as("SELECT slug, id FROM grammar_points")
            .fetch_all(pool)
            .await?;
    let slug_map: HashMap<String, i64> = slug_rows.into_iter().collect();

    let now = Utc::now();
    let mut tx = pool.begin().await?;
    let mut imported = 0usize;
    let mut skipped = 0usize;

    for row in rows_to_import {
        let Some(&grammar_id) = slug_map.get(row.slug.trim()) else {
            skipped += 1;
            if errors.len() < 20 {
                errors.push(format!("Unknown grammar slug '{}'", row.slug));
            }
            continue;
        };

        let srs_stage = row.srs_stage.clamp(0, 12);
        let ghost_level = row.ghost_level.clamp(0, 4);
        let correct_count = row.correct_count.max(0);
        let incorrect_count = row.incorrect_count.max(0);
        let streak = row.streak.max(0);

        let next_review_at = row
            .next_review_at
            .as_deref()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|d| d.with_timezone(&Utc))
            .or_else(|| {
                if srs_stage > 0 && srs_stage < 12 {
                    srs_interval_hours(srs_stage).map(|h| now + Duration::hours(h))
                } else {
                    None
                }
            });

        let ghost_due_at = row
            .ghost_due_at
            .as_deref()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|d| d.with_timezone(&Utc))
            .or_else(|| {
                if ghost_level > 0 {
                    Some(now + Duration::hours(4))
                } else {
                    None
                }
            });

        sqlx::query(
            "INSERT INTO user_progress
                 (user_id, grammar_id, srs_stage, next_review_at, ghost_level, ghost_due_at, correct_count, incorrect_count, streak, user_notes)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
             ON CONFLICT (user_id, grammar_id) DO UPDATE SET
                 srs_stage = EXCLUDED.srs_stage,
                 next_review_at = EXCLUDED.next_review_at,
                 ghost_level = EXCLUDED.ghost_level,
                 ghost_due_at = EXCLUDED.ghost_due_at,
                 correct_count = EXCLUDED.correct_count,
                 incorrect_count = EXCLUDED.incorrect_count,
                 streak = EXCLUDED.streak,
                 user_notes = EXCLUDED.user_notes",
        )
        .bind(user_id)
        .bind(grammar_id)
        .bind(srs_stage)
        .bind(next_review_at)
        .bind(ghost_level)
        .bind(ghost_due_at)
        .bind(correct_count)
        .bind(incorrect_count)
        .bind(streak)
        .bind(&row.user_notes)
        .execute(&mut *tx)
        .await?;

        imported += 1;
    }

    tx.commit().await?;

    Ok(ImportProgressResponse {
        imported,
        skipped,
        errors,
    })
}

pub async fn compute_dashboard(pool: &PgPool, user_id: i64) -> Result<DashboardStats> {
    let now = Utc::now();
    let prefs = load_user_prefs(pool, user_id).await?;

    let (regular_due, ghost_due): (i64, i64) = if prefs.vacation_mode {
        (0, 0)
    } else {
        let regular: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM user_progress
             WHERE user_id = $1 AND srs_stage > 0 AND srs_stage < 12 AND next_review_at IS NOT NULL AND next_review_at <= $2",
        )
        .bind(user_id)
        .bind(now)
        .fetch_one(pool)
        .await?;

        let ghost: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM user_progress
             WHERE user_id = $1 AND ghost_level > 0 AND ghost_due_at IS NOT NULL AND ghost_due_at <= $2",
        )
        .bind(user_id)
        .bind(now)
        .fetch_one(pool)
        .await?;
        (regular, ghost)
    };

    let lessons_available: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)
         FROM grammar_points g
         LEFT JOIN user_progress p ON p.grammar_id = g.id AND p.user_id = $1
         WHERE COALESCE(p.srs_stage, 0) = 0",
    )
    .bind(user_id)
    .fetch_one(pool)
    .await?;

    let total_learned: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM user_progress WHERE user_id = $1 AND srs_stage > 0")
            .bind(user_id)
            .fetch_one(pool)
            .await?;

    let total_grammar: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM grammar_points")
        .fetch_one(pool)
        .await?;

    let leech_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM user_progress
         WHERE user_id = $1
           AND incorrect_count >= 2
           AND (correct_count + incorrect_count) >= 3
           AND (incorrect_count::DOUBLE PRECISION / (correct_count + incorrect_count)) >= 0.35",
    )
    .bind(user_id)
    .fetch_one(pool)
    .await?;

    // 7-day accuracy & today count
    let week_ago = now - Duration::days(7);
    let (reviews_7d, correct_7d): (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*),
                COALESCE(SUM(CASE WHEN correct THEN 1 ELSE 0 END), 0)::BIGINT
         FROM review_logs
         WHERE user_id = $1 AND reviewed_at >= $2",
    )
    .bind(user_id)
    .bind(week_ago)
    .fetch_one(pool)
    .await?;

    let accuracy_7d = if reviews_7d > 0 {
        ((correct_7d as f64 / reviews_7d as f64) * 100.0).round() as u8
    } else {
        100
    };

    let today_start = now
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc();
    let today_reviews: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM review_logs WHERE user_id = $1 AND reviewed_at >= $2",
    )
    .bind(user_id)
    .bind(today_start)
    .fetch_one(pool)
    .await?;

    // Consecutive daily study streak (anchored at vacation_started_at while Vacation Mode is active)
    let streak_anchor = if prefs.vacation_mode {
        prefs.vacation_started_at.unwrap_or(now)
    } else {
        now
    };
    let streak_window_start = (streak_anchor.date_naive() - Duration::days(365))
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc();
    let review_days: Vec<NaiveDate> = sqlx::query_scalar(
        "SELECT DISTINCT (reviewed_at AT TIME ZONE 'UTC')::DATE AS review_day
         FROM review_logs
         WHERE user_id = $1 AND reviewed_at >= $2",
    )
    .bind(user_id)
    .bind(streak_window_start)
    .fetch_all(pool)
    .await?;
    let review_day_set: HashSet<NaiveDate> = review_days.into_iter().collect();

    let mut streak_days = 0;
    let anchor_date = streak_anchor.date_naive();
    for d in 0..365 {
        let day = anchor_date - Duration::days(d);
        if review_day_set.contains(&day) {
            streak_days += 1;
        } else if d > 0 {
            break;
        }
    }

    // JLPT progress
    let jlpt_rows: Vec<(String, i64, i64, i64)> = sqlx::query_as(
        "SELECT g.jlpt_level,
                COUNT(*),
                COALESCE(SUM(CASE WHEN COALESCE(p.srs_stage, 0) > 0 THEN 1 ELSE 0 END), 0)::BIGINT,
                COALESCE(SUM(CASE WHEN COALESCE(p.srs_stage, 0) >= 7 THEN 1 ELSE 0 END), 0)::BIGINT
         FROM grammar_points g
         LEFT JOIN user_progress p ON p.grammar_id = g.id AND p.user_id = $1
         GROUP BY g.jlpt_level",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    let jlpt_map: HashMap<String, (i64, i64, i64)> = jlpt_rows
        .into_iter()
        .map(|(lvl, total, learned, mastered)| (lvl, (total, learned, mastered)))
        .collect();

    let mut jlpt_progress = Vec::with_capacity(5);
    for lvl in ["N5", "N4", "N3", "N2", "N1"] {
        let (total, learned, mastered) = jlpt_map.get(lvl).copied().unwrap_or((0, 0, 0));
        let percent = if total > 0 {
            ((learned as f64 / total as f64) * 100.0).round() as u8
        } else {
            0
        };
        jlpt_progress.push(JlptProgress {
            level: lvl.to_string(),
            learned: learned as i32,
            total: total as i32,
            mastered: mastered as i32,
            percent,
        });
    }

    // 24h forecast buckets
    #[derive(FromRow)]
    struct DueTimesRow {
        next_review_at: Option<DateTime<Utc>>,
        ghost_due_at: Option<DateTime<Utc>>,
    }

    let due_rows: Vec<DueTimesRow> = if prefs.vacation_mode {
        Vec::new()
    } else {
        sqlx::query_as(
            "SELECT next_review_at, ghost_due_at
             FROM user_progress
             WHERE user_id = $1 AND srs_stage > 0 AND srs_stage < 12",
        )
        .bind(user_id)
        .fetch_all(pool)
        .await?
    };

    let mut due_times: Vec<DateTime<Utc>> = Vec::new();
    for row in due_rows {
        if let Some(dt) = row.next_review_at {
            due_times.push(dt);
        }
        if let Some(dt) = row.ghost_due_at {
            due_times.push(dt);
        }
    }

    let bucket_specs: [(&str, i64, i64); 8] = [
        ("Now", -99999, 0),
        ("+3h", 0, 3),
        ("+6h", 3, 6),
        ("+9h", 6, 9),
        ("+12h", 9, 12),
        ("+16h", 12, 16),
        ("+20h", 16, 20),
        ("+24h", 20, 24),
    ];
    let mut forecast_24h = Vec::with_capacity(bucket_specs.len());
    for (label, lo, hi) in bucket_specs {
        let count = due_times
            .iter()
            .filter(|dt| {
                let diff_h = (**dt - now).num_minutes() as f64 / 60.0;
                if lo == -99999 {
                    diff_h <= 0.0
                } else {
                    diff_h > lo as f64 && diff_h <= hi as f64
                }
            })
            .count() as i32;
        forecast_24h.push(ForecastBucket {
            label: label.to_string(),
            count,
        });
    }

    // SRS Breakdown
    let stage_rows: Vec<(i32, i64)> = sqlx::query_as(
        "SELECT srs_stage, COUNT(*)
         FROM user_progress
         WHERE user_id = $1 AND srs_stage >= 1
         GROUP BY srs_stage",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    let tiers: [(&str, &str, i32, i32); 5] = [
        ("Apprentice", "1–4", 1, 4),
        ("Guru", "5–6", 5, 6),
        ("Master", "7–8", 7, 8),
        ("Enlightened", "9–10", 9, 10),
        ("Burned", "11–12", 11, 12),
    ];
    let mut srs_breakdown = Vec::with_capacity(tiers.len());
    for (tier, range, lo, hi) in tiers {
        let count: i64 = stage_rows
            .iter()
            .filter(|(stage, _)| *stage >= lo && *stage <= hi)
            .map(|(_, c)| *c)
            .sum();
        srs_breakdown.push(SrsTierCount {
            tier: tier.to_string(),
            range: range.to_string(),
            count: count as i32,
        });
    }

    let wanikani = load_wanikani_status(pool, user_id).await?;
    let onboarding = OnboardingState {
        completed: prefs.onboarding_completed,
        preferred_study_path: prefs.preferred_study_path.clone(),
        placement_jlpt: prefs.placement_jlpt,
        daily_goal: prefs.daily_goal,
        content_version: CONTENT_SCHEMA_VERSION,
    };
    let srs_settings = SrsSettings {
        vacation_mode: prefs.vacation_mode,
        vacation_started_at: prefs.vacation_started_at.map(|d| d.to_rfc3339()),
        ghost_intensity: GhostIntensity::parse(&prefs.ghost_intensity)
            .as_str()
            .to_string(),
        daily_goal: prefs.daily_goal,
        preferred_study_path: prefs.preferred_study_path,
    };

    Ok(DashboardStats {
        reviews_due: (regular_due + ghost_due) as i32,
        ghost_reviews_due: ghost_due as i32,
        lessons_available: lessons_available as i32,
        total_learned: total_learned as i32,
        total_grammar: total_grammar as i32,
        accuracy_7d,
        reviews_7d: reviews_7d as i32,
        streak_days,
        daily_goal: prefs.daily_goal,
        today_reviews: today_reviews as i32,
        leech_count: leech_count as i32,
        jlpt_progress,
        forecast_24h,
        srs_breakdown,
        wanikani,
        onboarding,
        srs_settings,
    })
}

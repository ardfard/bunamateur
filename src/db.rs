use crate::models::{
    ConjugationItem, DashboardStats, DetectedGrammar, ForecastBucket, GrammarPointDetail,
    GrammarPointSummary, GrammarSentence, JlptProgress, ReadingPassage, ReviewQueueItem,
    SrsTierCount, WaniKaniStatus,
};
use crate::seed_data::{SEED_CONJUGATIONS, SEED_GRAMMAR, SEED_PASSAGES};
use crate::srs::{SrsTransition, srs_stage_label};
use crate::wanikani::{WaniKaniSyncResult, compute_kanji_readiness, strip_ruby_markup};
use anyhow::{Context, Result};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use sqlx::{FromRow, PgPool, postgres::PgPoolOptions};
use std::collections::{HashMap, HashSet};
use std::time::Duration as StdDuration;

pub const DEFAULT_DATABASE_URL: &str = "postgres://ardfard@127.0.0.1:5432/bunamateur_dev";

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
}

impl AppState {
    pub async fn connect(database_url: &str) -> Result<Self> {
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

        Self::from_pool(pool).await
    }

    pub async fn from_pool(pool: PgPool) -> Result<Self> {
        run_migrations(&pool).await?;
        seed_content(&pool).await?;
        Ok(Self { pool })
    }
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

/// Idempotently upserts static curriculum content (grammar points, sentences, reading passages,
/// conjugation drills) without overwriting existing user progress, notes, review logs, or WaniKani profile.
pub async fn seed_content(pool: &PgPool) -> Result<()> {
    let mut tx = pool.begin().await?;

    sqlx::query(
        "INSERT INTO wk_profile (id, smart_furigana, highlight_known_kanji)
         VALUES (1, TRUE, TRUE)
         ON CONFLICT (id) DO NOTHING",
    )
    .execute(&mut *tx)
    .await?;

    let existing_grammar_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM grammar_points")
        .fetch_one(&mut *tx)
        .await?;
    let is_fresh_db = existing_grammar_count == 0;

    let now = Utc::now();
    let mut seeded_grammar_ids: Vec<i64> = Vec::with_capacity(SEED_GRAMMAR.len());

    for g in SEED_GRAMMAR {
        let patterns: Vec<String> = g.match_patterns.iter().map(|s| (*s).to_string()).collect();

        let grammar_id: i64 = sqlx::query_scalar(
            "INSERT INTO grammar_points
                 (slug, title, reading, meaning, jlpt_level, study_path, structure, nuance, caution, confusable_with, pitch_note, match_patterns)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
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
                 match_patterns = EXCLUDED.match_patterns
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
        .fetch_one(&mut *tx)
        .await?;

        seeded_grammar_ids.push(grammar_id);

        for (pos, s) in g.sentences.iter().enumerate() {
            let alts: Vec<String> = s.alt_answers.iter().map(|a| (*a).to_string()).collect();
            sqlx::query(
                "INSERT INTO grammar_sentences
                     (grammar_id, position, jp_before, jp_after, answer, alt_answers, hint, english, nuance_note)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                 ON CONFLICT (grammar_id, position) DO UPDATE SET
                     jp_before = EXCLUDED.jp_before,
                     jp_after = EXCLUDED.jp_after,
                     answer = EXCLUDED.answer,
                     alt_answers = EXCLUDED.alt_answers,
                     hint = EXCLUDED.hint,
                     english = EXCLUDED.english,
                     nuance_note = EXCLUDED.nuance_note",
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
            .execute(&mut *tx)
            .await?;
        }

        // Preserve existing user progress on subsequent runs; seed initial progress only on a fresh DB.
        let (init_srs, next_review, init_ghost, ghost_due, init_correct, init_incorrect, init_streak) =
            if is_fresh_db {
                let next_review = g
                    .initial_due_offset_hours
                    .map(|h| now + Duration::hours(h));
                let ghost_due = if g.initial_ghost > 0 {
                    Some(now - Duration::minutes(30))
                } else {
                    None
                };
                let init_correct = if g.initial_srs > 0 {
                    g.initial_srs * 2
                } else {
                    0
                };
                let init_incorrect = if g.initial_ghost > 0 { 2 } else { 0 };
                (
                    g.initial_srs,
                    next_review,
                    g.initial_ghost,
                    ghost_due,
                    init_correct,
                    init_incorrect,
                    g.initial_srs.max(0),
                )
            } else {
                (0, None, 0, None, 0, 0, 0)
            };

        sqlx::query(
            "INSERT INTO user_progress
                 (grammar_id, srs_stage, next_review_at, ghost_level, ghost_due_at, correct_count, incorrect_count, streak)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (grammar_id) DO NOTHING",
        )
        .bind(grammar_id)
        .bind(init_srs)
        .bind(next_review)
        .bind(init_ghost)
        .bind(ghost_due)
        .bind(init_correct)
        .bind(init_incorrect)
        .bind(init_streak)
        .execute(&mut *tx)
        .await?;
    }

    // Seed recent review logs only on a brand-new database
    if is_fresh_db && !seeded_grammar_ids.is_empty() {
        let sample_ids: Vec<i64> = seeded_grammar_ids.iter().copied().take(6).collect();
        for day_offset in 0..5 {
            let ts = now - Duration::days(day_offset) - Duration::hours(1);
            for (idx, &gid) in sample_ids.iter().enumerate() {
                let correct = !(day_offset == 1 && idx == 4);
                sqlx::query(
                    "INSERT INTO review_logs (grammar_id, correct, mode, is_ghost, reviewed_at)
                     VALUES ($1, $2, 'review', FALSE, $3)",
                )
                .bind(gid)
                .bind(correct)
                .bind(ts)
                .execute(&mut *tx)
                .await?;
            }
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

    tx.commit().await?;
    Ok(())
}

pub async fn load_known_kanji(pool: &PgPool) -> Result<HashSet<char>> {
    let rows: Vec<String> =
        sqlx::query_scalar("SELECT characters FROM wk_subjects WHERE subject_type = 'kanji'")
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
}

pub async fn load_wanikani_status(pool: &PgPool) -> Result<WaniKaniStatus> {
    let profile: WkProfileRow = sqlx::query_as(
        "SELECT api_token, is_demo, username, level, lessons_available, reviews_available,
                smart_furigana, highlight_known_kanji, last_synced_at
         FROM wk_profile
         WHERE id = 1",
    )
    .fetch_one(pool)
    .await?;

    let kanji_known: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM wk_subjects WHERE subject_type = 'kanji'")
            .fetch_one(pool)
            .await?;
    let vocab_known: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM wk_subjects WHERE subject_type = 'vocabulary'")
            .fetch_one(pool)
            .await?;

    let known_kanji_list: Vec<String> = sqlx::query_scalar(
        "SELECT characters FROM wk_subjects WHERE subject_type = 'kanji' ORDER BY level, subject_id",
    )
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
    })
}

pub async fn get_wanikani_credentials(pool: &PgPool) -> Result<(Option<String>, bool)> {
    let row: (Option<String>, bool) =
        sqlx::query_as("SELECT api_token, is_demo FROM wk_profile WHERE id = 1")
            .fetch_one(pool)
            .await?;
    Ok(row)
}

pub async fn save_wanikani_sync(
    pool: &PgPool,
    api_token: Option<&str>,
    is_demo: bool,
    sync: &WaniKaniSyncResult,
) -> Result<WaniKaniStatus> {
    let now = Utc::now();
    let mut tx = pool.begin().await?;

    sqlx::query(
        "UPDATE wk_profile
         SET api_token = $1,
             is_demo = $2,
             username = $3,
             level = $4,
             lessons_available = $5,
             reviews_available = $6,
             last_synced_at = $7
         WHERE id = 1",
    )
    .bind(api_token)
    .bind(is_demo)
    .bind(&sync.username)
    .bind(sync.level)
    .bind(sync.lessons_available)
    .bind(sync.reviews_available)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    sqlx::query("DELETE FROM wk_subjects")
        .execute(&mut *tx)
        .await?;

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
            "INSERT INTO wk_subjects (subject_id, subject_type, characters, level, srs_stage)
             SELECT * FROM UNNEST($1::BIGINT[], $2::TEXT[], $3::TEXT[], $4::INTEGER[], $5::INTEGER[])
             ON CONFLICT (subject_id) DO UPDATE SET
                 subject_type = EXCLUDED.subject_type,
                 characters = EXCLUDED.characters,
                 level = EXCLUDED.level,
                 srs_stage = EXCLUDED.srs_stage",
        )
        .bind(&ids)
        .bind(&types)
        .bind(&chars)
        .bind(&levels)
        .bind(&stages)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    load_wanikani_status(pool).await
}

pub async fn disconnect_wanikani(pool: &PgPool) -> Result<WaniKaniStatus> {
    let mut tx = pool.begin().await?;
    sqlx::query(
        "UPDATE wk_profile
         SET api_token = NULL,
             is_demo = FALSE,
             username = NULL,
             level = 0,
             lessons_available = 0,
             reviews_available = 0,
             last_synced_at = NULL
         WHERE id = 1",
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query("DELETE FROM wk_subjects")
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;
    load_wanikani_status(pool).await
}

pub async fn update_wanikani_settings(
    pool: &PgPool,
    smart_furigana: bool,
    highlight_known_kanji: bool,
) -> Result<WaniKaniStatus> {
    sqlx::query(
        "UPDATE wk_profile
         SET smart_furigana = $1, highlight_known_kanji = $2
         WHERE id = 1",
    )
    .bind(smart_furigana)
    .bind(highlight_known_kanji)
    .execute(pool)
    .await?;

    load_wanikani_status(pool).await
}

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
    jlpt_filter: Option<&str>,
    query: Option<&str>,
    status_filter: Option<&str>,
) -> Result<Vec<GrammarPointSummary>> {
    let known_kanji = load_known_kanji(pool).await?;

    let rows: Vec<GrammarSummaryRow> = sqlx::query_as(
        "SELECT g.id, g.slug, g.title, g.reading, g.meaning, g.jlpt_level, g.study_path, g.structure,
                p.srs_stage, p.next_review_at, p.ghost_level, p.correct_count, p.incorrect_count,
                COALESCE(STRING_AGG(s.jp_before || s.answer || s.jp_after, ' ' ORDER BY s.position, s.id), '') AS combined_sentences
         FROM grammar_points g
         JOIN user_progress p ON p.grammar_id = g.id
         LEFT JOIN grammar_sentences s ON s.grammar_id = g.id
         GROUP BY g.id, p.grammar_id
         ORDER BY CASE g.jlpt_level WHEN 'N5' THEN 1 WHEN 'N4' THEN 2 WHEN 'N3' THEN 3 WHEN 'N2' THEN 4 ELSE 5 END, g.id",
    )
    .fetch_all(pool)
    .await?;

    let q_lower = query.unwrap_or("").trim().to_lowercase();
    let jlpt = jlpt_filter.unwrap_or("ALL").to_uppercase();
    let status = status_filter.unwrap_or("all").to_lowercase();

    let mut items = Vec::new();
    for row in rows {
        if jlpt != "ALL" && row.jlpt_level != jlpt {
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
    grammar_id: i64,
) -> Result<Option<GrammarPointDetail>> {
    let known_kanji = load_known_kanji(pool).await?;

    let row: Option<GrammarDetailRow> = sqlx::query_as(
        "SELECT g.id, g.slug, g.title, g.reading, g.meaning, g.jlpt_level, g.study_path,
                g.structure, g.nuance, g.caution, g.confusable_with, g.pitch_note,
                p.user_notes, p.srs_stage, p.correct_count, p.incorrect_count, p.streak, p.ghost_level, p.next_review_at
         FROM grammar_points g
         JOIN user_progress p ON p.grammar_id = g.id
         WHERE g.id = $1",
    )
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

pub async fn update_grammar_notes(pool: &PgPool, grammar_id: i64, notes: &str) -> Result<()> {
    sqlx::query("UPDATE user_progress SET user_notes = $1 WHERE grammar_id = $2")
        .bind(notes)
        .bind(grammar_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn reset_grammar_progress(pool: &PgPool, grammar_id: i64) -> Result<()> {
    sqlx::query(
        "UPDATE user_progress
         SET srs_stage = 0,
             next_review_at = NULL,
             ghost_level = 0,
             ghost_due_at = NULL,
             streak = 0
         WHERE grammar_id = $1",
    )
    .bind(grammar_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn start_lesson(
    pool: &PgPool,
    grammar_id: i64,
    next_due: DateTime<Utc>,
) -> Result<()> {
    sqlx::query(
        "UPDATE user_progress
         SET srs_stage = CASE WHEN srs_stage = 0 THEN 1 ELSE srs_stage END,
             next_review_at = COALESCE(next_review_at, $1)
         WHERE grammar_id = $2",
    )
    .bind(next_due)
    .bind(grammar_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_sentence_answers(
    pool: &PgPool,
    sentence_id: i64,
) -> Result<Option<(String, Vec<String>)>> {
    let row: Option<(String, Vec<String>)> =
        sqlx::query_as("SELECT answer, alt_answers FROM grammar_sentences WHERE id = $1")
            .bind(sentence_id)
            .fetch_optional(pool)
            .await?;
    Ok(row)
}

pub async fn get_user_progress_stages(
    pool: &PgPool,
    grammar_id: i64,
) -> Result<Option<(i32, i32)>> {
    let row: Option<(i32, i32)> =
        sqlx::query_as("SELECT srs_stage, ghost_level FROM user_progress WHERE grammar_id = $1")
            .bind(grammar_id)
            .fetch_optional(pool)
            .await?;
    Ok(row)
}

pub async fn record_review_submission(
    pool: &PgPool,
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

    if is_ghost {
        sqlx::query(
            "UPDATE user_progress
             SET ghost_level = $1,
                 ghost_due_at = $2,
                 correct_count = correct_count + $3,
                 incorrect_count = incorrect_count + $4
             WHERE grammar_id = $5",
        )
        .bind(transition.new_ghost_level)
        .bind(transition.ghost_due_at)
        .bind(correct_delta)
        .bind(incorrect_delta)
        .bind(grammar_id)
        .execute(&mut *tx)
        .await?;
    } else {
        sqlx::query(
            "UPDATE user_progress
             SET srs_stage = $1,
                 next_review_at = $2,
                 ghost_level = $3,
                 ghost_due_at = COALESCE($4, ghost_due_at),
                 correct_count = correct_count + $5,
                 incorrect_count = incorrect_count + $6,
                 streak = CASE WHEN $7 THEN streak + 1 ELSE 0 END
             WHERE grammar_id = $8",
        )
        .bind(transition.new_stage)
        .bind(transition.next_review_at)
        .bind(transition.new_ghost_level)
        .bind(transition.ghost_due_at)
        .bind(correct_delta)
        .bind(incorrect_delta)
        .bind(correct)
        .bind(grammar_id)
        .execute(&mut *tx)
        .await?;
    }

    let log_id: i64 = sqlx::query_scalar(
        "INSERT INTO review_logs (grammar_id, correct, mode, is_ghost, reviewed_at)
         VALUES ($1, $2, $3, $4, $5)
         RETURNING id",
    )
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
        "UPDATE user_progress
         SET srs_stage = $1,
             next_review_at = $2,
             ghost_level = $3,
             ghost_due_at = $4
         WHERE grammar_id = $5",
    )
    .bind(previous_srs_stage)
    .bind(restore_due)
    .bind(previous_ghost_level)
    .bind(restore_ghost_due)
    .bind(grammar_id)
    .execute(&mut *tx)
    .await?;

    if let Some(lid) = review_log_id {
        sqlx::query("DELETE FROM review_logs WHERE id = $1")
            .bind(lid)
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
    mode: &str,
    jlpt_filter: Option<&str>,
    limit: usize,
) -> Result<Vec<ReviewQueueItem>> {
    let now = Utc::now();
    let jlpt = jlpt_filter.unwrap_or("ALL").to_uppercase();

    let mut candidates: Vec<(QueueGrammarRow, bool, &'static str)> = Vec::new();
    let mut seen_grammar: HashSet<i64> = HashSet::new();

    let mut accept = |rows: Vec<QueueGrammarRow>, is_ghost_fn: fn(i32) -> bool, item_mode: &'static str| -> bool {
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
                "SELECT g.id, g.title, g.reading, g.meaning, g.jlpt_level, g.structure, g.nuance, g.caution, g.confusable_with, p.srs_stage, p.ghost_level
                 FROM grammar_points g
                 JOIN user_progress p ON p.grammar_id = g.id
                 WHERE p.ghost_level > 0
                 ORDER BY p.ghost_due_at ASC NULLS LAST, g.id",
            )
            .fetch_all(pool)
            .await?;
            accept(rows, |_| true, "review");
        }
        "lesson" => {
            let rows: Vec<QueueGrammarRow> = sqlx::query_as(
                "SELECT g.id, g.title, g.reading, g.meaning, g.jlpt_level, g.structure, g.nuance, g.caution, g.confusable_with, p.srs_stage, p.ghost_level
                 FROM grammar_points g
                 JOIN user_progress p ON p.grammar_id = g.id
                 WHERE p.srs_stage = 0
                 ORDER BY CASE g.jlpt_level WHEN 'N5' THEN 1 WHEN 'N4' THEN 2 WHEN 'N3' THEN 3 WHEN 'N2' THEN 4 ELSE 5 END, g.id",
            )
            .fetch_all(pool)
            .await?;
            accept(rows, |_| false, "lesson");
        }
        "cram" => {
            let rows: Vec<QueueGrammarRow> = sqlx::query_as(
                "SELECT g.id, g.title, g.reading, g.meaning, g.jlpt_level, g.structure, g.nuance, g.caution, g.confusable_with, p.srs_stage, p.ghost_level
                 FROM grammar_points g
                 JOIN user_progress p ON p.grammar_id = g.id
                 ORDER BY p.ghost_level DESC, p.incorrect_count DESC, p.srs_stage DESC, g.id",
            )
            .fetch_all(pool)
            .await?;
            accept(rows, |gl| gl > 0, "cram");
        }
        _ => {
            // Due Ghost Reviews first, then due regular SRS reviews
            let ghost_rows: Vec<QueueGrammarRow> = sqlx::query_as(
                "SELECT g.id, g.title, g.reading, g.meaning, g.jlpt_level, g.structure, g.nuance, g.caution, g.confusable_with, p.srs_stage, p.ghost_level
                 FROM grammar_points g
                 JOIN user_progress p ON p.grammar_id = g.id
                 WHERE p.ghost_level > 0 AND p.ghost_due_at IS NOT NULL AND p.ghost_due_at <= $1
                 ORDER BY p.ghost_due_at ASC, g.id",
            )
            .bind(now)
            .fetch_all(pool)
            .await?;
            if accept(ghost_rows, |_| true, "review") {
                let regular_rows: Vec<QueueGrammarRow> = sqlx::query_as(
                    "SELECT g.id, g.title, g.reading, g.meaning, g.jlpt_level, g.structure, g.nuance, g.caution, g.confusable_with, p.srs_stage, p.ghost_level
                     FROM grammar_points g
                     JOIN user_progress p ON p.grammar_id = g.id
                     WHERE p.srs_stage > 0 AND p.srs_stage < 12 AND p.next_review_at IS NOT NULL AND p.next_review_at <= $1
                     ORDER BY p.next_review_at ASC, g.id",
                )
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

async fn load_grammar_match_rows(pool: &PgPool) -> Result<Vec<GrammarMatchRow>> {
    let rows: Vec<GrammarMatchRow> = sqlx::query_as(
        "SELECT g.id, g.title, g.meaning, g.jlpt_level, g.match_patterns, p.srs_stage
         FROM grammar_points g
         JOIN user_progress p ON p.grammar_id = g.id
         ORDER BY g.id",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

fn match_grammar_patterns(rows: &[GrammarMatchRow], text: &str) -> Vec<DetectedGrammar> {
    let plain = strip_ruby_markup(text);
    let mut found = Vec::new();
    for row in rows {
        if let Some(matched) = row
            .match_patterns
            .iter()
            .find(|pat| !pat.is_empty() && plain.contains(pat.as_str()))
        {
            found.push(DetectedGrammar {
                grammar_id: row.id,
                title: row.title.clone(),
                meaning: row.meaning.clone(),
                jlpt_level: row.jlpt_level.clone(),
                srs_stage: row.srs_stage,
                srs_label: srs_stage_label(row.srs_stage),
                matched_token: matched.clone(),
            });
        }
    }
    found
}

pub async fn detect_grammar_in_text(pool: &PgPool, text: &str) -> Result<Vec<DetectedGrammar>> {
    let rows = load_grammar_match_rows(pool).await?;
    Ok(match_grammar_patterns(&rows, text))
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

pub async fn list_reading_passages(pool: &PgPool) -> Result<Vec<ReadingPassage>> {
    let known_kanji = load_known_kanji(pool).await?;
    let grammar_rows = load_grammar_match_rows(pool).await?;

    let rows: Vec<ReadingPassageRow> = sqlx::query_as(
        "SELECT id, title, jlpt_level, topic, content_ruby, english
         FROM reading_passages
         ORDER BY id",
    )
    .fetch_all(pool)
    .await?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let (_, _, wk_pct, _) = compute_kanji_readiness(&row.content_ruby, &known_kanji);
        let detected = match_grammar_patterns(&grammar_rows, &row.content_ruby);
        out.push(ReadingPassage {
            id: row.id,
            title: row.title,
            jlpt_level: row.jlpt_level,
            topic: row.topic,
            content_ruby: row.content_ruby,
            english: row.english,
            wk_readiness_pct: wk_pct,
            detected_grammar: detected,
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

pub async fn compute_dashboard(pool: &PgPool) -> Result<DashboardStats> {
    let now = Utc::now();

    let regular_due: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM user_progress
         WHERE srs_stage > 0 AND srs_stage < 12 AND next_review_at IS NOT NULL AND next_review_at <= $1",
    )
    .bind(now)
    .fetch_one(pool)
    .await?;

    let ghost_due: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM user_progress
         WHERE ghost_level > 0 AND ghost_due_at IS NOT NULL AND ghost_due_at <= $1",
    )
    .bind(now)
    .fetch_one(pool)
    .await?;

    let lessons_available: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM user_progress WHERE srs_stage = 0")
            .fetch_one(pool)
            .await?;

    let total_learned: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM user_progress WHERE srs_stage > 0")
            .fetch_one(pool)
            .await?;

    let total_grammar: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM grammar_points")
        .fetch_one(pool)
        .await?;

    let leech_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM user_progress
         WHERE incorrect_count >= 2
           AND (correct_count + incorrect_count) >= 3
           AND (incorrect_count::DOUBLE PRECISION / (correct_count + incorrect_count)) >= 0.35",
    )
    .fetch_one(pool)
    .await?;

    // 7-day accuracy & today count
    let week_ago = now - Duration::days(7);
    let (reviews_7d, correct_7d): (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*),
                COALESCE(SUM(CASE WHEN correct THEN 1 ELSE 0 END), 0)::BIGINT
         FROM review_logs
         WHERE reviewed_at >= $1",
    )
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
    let today_reviews: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM review_logs WHERE reviewed_at >= $1")
            .bind(today_start)
            .fetch_one(pool)
            .await?;

    // Consecutive daily study streak in a single query
    let streak_window_start = (now.date_naive() - Duration::days(365))
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc();
    let review_days: Vec<NaiveDate> = sqlx::query_scalar(
        "SELECT DISTINCT (reviewed_at AT TIME ZONE 'UTC')::DATE AS review_day
         FROM review_logs
         WHERE reviewed_at >= $1",
    )
    .bind(streak_window_start)
    .fetch_all(pool)
    .await?;
    let review_day_set: HashSet<NaiveDate> = review_days.into_iter().collect();

    let mut streak_days = 0;
    let today_date = now.date_naive();
    for d in 0..365 {
        let day = today_date - Duration::days(d);
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
                COALESCE(SUM(CASE WHEN p.srs_stage > 0 THEN 1 ELSE 0 END), 0)::BIGINT,
                COALESCE(SUM(CASE WHEN p.srs_stage >= 7 THEN 1 ELSE 0 END), 0)::BIGINT
         FROM grammar_points g
         JOIN user_progress p ON p.grammar_id = g.id
         GROUP BY g.jlpt_level",
    )
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

    let due_rows: Vec<DueTimesRow> = sqlx::query_as(
        "SELECT next_review_at, ghost_due_at
         FROM user_progress
         WHERE srs_stage > 0 AND srs_stage < 12",
    )
    .fetch_all(pool)
    .await?;

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
         WHERE srs_stage >= 1
         GROUP BY srs_stage",
    )
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

    let wanikani = load_wanikani_status(pool).await?;

    Ok(DashboardStats {
        reviews_due: (regular_due + ghost_due) as i32,
        ghost_reviews_due: ghost_due as i32,
        lessons_available: lessons_available as i32,
        total_learned: total_learned as i32,
        total_grammar: total_grammar as i32,
        accuracy_7d,
        reviews_7d: reviews_7d as i32,
        streak_days,
        daily_goal: 20,
        today_reviews: today_reviews as i32,
        leech_count: leech_count as i32,
        jlpt_progress,
        forecast_24h,
        srs_breakdown,
        wanikani,
    })
}

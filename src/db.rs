use crate::models::{
    ConjugationItem, DetectedGrammar, ForecastBucket, GrammarPointDetail, GrammarPointSummary,
    GrammarSentence, JlptProgress, ReadingPassage, ReviewQueueItem, SrsTierCount, WaniKaniStatus,
};
use crate::seed_data::{SEED_CONJUGATIONS, SEED_GRAMMAR, SEED_PASSAGES};
use crate::srs::srs_stage_label;
use crate::wanikani::{WaniKaniSyncResult, compute_kanji_readiness, strip_ruby_markup};
use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Mutex<Connection>>,
}

impl AppState {
    pub fn open(db_path: &str) -> Result<Self> {
        let conn = Connection::open(db_path)?;
        conn.execute_batch(
            "
            PRAGMA journal_mode = WAL;
            PRAGMA foreign_keys = ON;

            CREATE TABLE IF NOT EXISTS grammar_points (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                slug TEXT NOT NULL UNIQUE,
                title TEXT NOT NULL,
                reading TEXT NOT NULL,
                meaning TEXT NOT NULL,
                jlpt_level TEXT NOT NULL,
                study_path TEXT NOT NULL,
                structure TEXT NOT NULL,
                nuance TEXT NOT NULL,
                caution TEXT NOT NULL,
                confusable_with TEXT NOT NULL,
                pitch_note TEXT NOT NULL,
                match_patterns TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS grammar_sentences (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                grammar_id INTEGER NOT NULL,
                jp_before TEXT NOT NULL,
                jp_after TEXT NOT NULL,
                answer TEXT NOT NULL,
                alt_answers TEXT NOT NULL,
                hint TEXT NOT NULL,
                english TEXT NOT NULL,
                nuance_note TEXT NOT NULL,
                FOREIGN KEY (grammar_id) REFERENCES grammar_points(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS user_progress (
                grammar_id INTEGER PRIMARY KEY,
                srs_stage INTEGER NOT NULL DEFAULT 0,
                next_review_at TEXT,
                ghost_level INTEGER NOT NULL DEFAULT 0,
                ghost_due_at TEXT,
                correct_count INTEGER NOT NULL DEFAULT 0,
                incorrect_count INTEGER NOT NULL DEFAULT 0,
                streak INTEGER NOT NULL DEFAULT 0,
                user_notes TEXT NOT NULL DEFAULT '',
                FOREIGN KEY (grammar_id) REFERENCES grammar_points(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS review_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                grammar_id INTEGER NOT NULL,
                correct INTEGER NOT NULL,
                mode TEXT NOT NULL,
                is_ghost INTEGER NOT NULL DEFAULT 0,
                reviewed_at TEXT NOT NULL,
                FOREIGN KEY (grammar_id) REFERENCES grammar_points(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS wk_profile (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                api_token TEXT,
                is_demo INTEGER NOT NULL DEFAULT 0,
                username TEXT,
                level INTEGER NOT NULL DEFAULT 0,
                lessons_available INTEGER NOT NULL DEFAULT 0,
                reviews_available INTEGER NOT NULL DEFAULT 0,
                smart_furigana INTEGER NOT NULL DEFAULT 1,
                highlight_known_kanji INTEGER NOT NULL DEFAULT 1,
                last_synced_at TEXT
            );

            CREATE TABLE IF NOT EXISTS wk_subjects (
                subject_id INTEGER PRIMARY KEY,
                subject_type TEXT NOT NULL,
                characters TEXT NOT NULL,
                level INTEGER NOT NULL,
                srs_stage INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS reading_passages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                title TEXT NOT NULL,
                jlpt_level TEXT NOT NULL,
                topic TEXT NOT NULL,
                content_ruby TEXT NOT NULL,
                english TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS conjugation_items (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                dictionary TEXT NOT NULL,
                reading TEXT NOT NULL,
                meaning TEXT NOT NULL,
                word_type TEXT NOT NULL,
                target_form TEXT NOT NULL,
                target_label TEXT NOT NULL,
                answer_kana TEXT NOT NULL,
                answer_kanji TEXT NOT NULL,
                rule_explanation TEXT NOT NULL,
                example_jp TEXT NOT NULL,
                example_en TEXT NOT NULL
            );
            ",
        )?;

        conn.execute(
            "INSERT OR IGNORE INTO wk_profile (id, smart_furigana, highlight_known_kanji) VALUES (1, 1, 1)",
            [],
        )?;

        seed_if_empty(&conn)?;

        Ok(Self {
            db: Arc::new(Mutex::new(conn)),
        })
    }
}

fn seed_if_empty(conn: &Connection) -> Result<()> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM grammar_points", [], |r| r.get(0))?;
    if count > 0 {
        return Ok(());
    }

    let now = Utc::now();

    for g in SEED_GRAMMAR {
        let patterns_json = serde_json::to_string(g.match_patterns)?;
        conn.execute(
            "INSERT INTO grammar_points
             (slug, title, reading, meaning, jlpt_level, study_path, structure, nuance, caution, confusable_with, pitch_note, match_patterns)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                g.slug,
                g.title,
                g.reading,
                g.meaning,
                g.jlpt_level,
                g.study_path,
                g.structure,
                g.nuance,
                g.caution,
                g.confusable_with,
                g.pitch_note,
                patterns_json,
            ],
        )?;
        let grammar_id = conn.last_insert_rowid();

        for s in g.sentences {
            let alt_json = serde_json::to_string(s.alt_answers)?;
            conn.execute(
                "INSERT INTO grammar_sentences
                 (grammar_id, jp_before, jp_after, answer, alt_answers, hint, english, nuance_note)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    grammar_id,
                    s.jp_before,
                    s.jp_after,
                    s.answer,
                    alt_json,
                    s.hint,
                    s.english,
                    s.nuance_note,
                ],
            )?;
        }

        let next_review = g
            .initial_due_offset_hours
            .map(|h| (now + Duration::hours(h)).to_rfc3339());
        let ghost_due = if g.initial_ghost > 0 {
            Some((now - Duration::minutes(30)).to_rfc3339())
        } else {
            None
        };
        let init_correct = if g.initial_srs > 0 { g.initial_srs * 2 } else { 0 };
        let init_incorrect = if g.initial_ghost > 0 { 2 } else { 0 };

        conn.execute(
            "INSERT INTO user_progress
             (grammar_id, srs_stage, next_review_at, ghost_level, ghost_due_at, correct_count, incorrect_count, streak)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                grammar_id,
                g.initial_srs,
                next_review,
                g.initial_ghost,
                ghost_due,
                init_correct,
                init_incorrect,
                g.initial_srs.max(0),
            ],
        )?;
    }

    // Seed a few recent review logs over the past 5 days so streak & 7d accuracy look realistic on first launch
    for day_offset in 0..5 {
        let ts = (now - Duration::days(day_offset) - Duration::hours(1)).to_rfc3339();
        for i in 1..=6 {
            let correct = if day_offset == 1 && i == 5 { 0 } else { 1 };
            conn.execute(
                "INSERT INTO review_logs (grammar_id, correct, mode, is_ghost, reviewed_at) VALUES (?1, ?2, 'review', 0, ?3)",
                params![i, correct, ts],
            )?;
        }
    }

    for p in SEED_PASSAGES {
        conn.execute(
            "INSERT INTO reading_passages (title, jlpt_level, topic, content_ruby, english) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![p.title, p.jlpt_level, p.topic, p.content_ruby, p.english],
        )?;
    }

    for c in SEED_CONJUGATIONS {
        conn.execute(
            "INSERT INTO conjugation_items
             (dictionary, reading, meaning, word_type, target_form, target_label, answer_kana, answer_kanji, rule_explanation, example_jp, example_en)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                c.dictionary,
                c.reading,
                c.meaning,
                c.word_type,
                c.target_form,
                c.target_label,
                c.answer_kana,
                c.answer_kanji,
                c.rule_explanation,
                c.example_jp,
                c.example_en,
            ],
        )?;
    }

    Ok(())
}

pub fn load_known_kanji(conn: &Connection) -> Result<HashSet<char>> {
    let mut stmt = conn.prepare("SELECT characters FROM wk_subjects WHERE subject_type = 'kanji'")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    let mut set = HashSet::new();
    for r in rows {
        if let Ok(chars) = r {
            for c in chars.chars() {
                set.insert(c);
            }
        }
    }
    Ok(set)
}

pub fn load_wanikani_status(conn: &Connection) -> Result<WaniKaniStatus> {
    let (token, is_demo, username, level, lessons, reviews, smart_furi, highlight, last_synced): (
        Option<String>,
        i32,
        Option<String>,
        i32,
        i32,
        i32,
        i32,
        i32,
        Option<String>,
    ) = conn.query_row(
        "SELECT api_token, is_demo, username, level, lessons_available, reviews_available, smart_furigana, highlight_known_kanji, last_synced_at FROM wk_profile WHERE id = 1",
        [],
        |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
                r.get(8)?,
            ))
        },
    )?;

    let kanji_known: usize = conn.query_row(
        "SELECT COUNT(*) FROM wk_subjects WHERE subject_type = 'kanji'",
        [],
        |r| r.get(0),
    )?;
    let vocab_known: usize = conn.query_row(
        "SELECT COUNT(*) FROM wk_subjects WHERE subject_type = 'vocabulary'",
        [],
        |r| r.get(0),
    )?;

    let mut stmt = conn.prepare("SELECT characters FROM wk_subjects WHERE subject_type = 'kanji'")?;
    let known_kanji_list: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .filter_map(|x| x.ok())
        .collect();

    let connected = token.is_some() || is_demo == 1;

    Ok(WaniKaniStatus {
        connected,
        is_demo: is_demo == 1,
        username,
        level,
        kanji_known,
        vocab_known,
        wk_lessons_available: lessons,
        wk_reviews_available: reviews,
        smart_furigana: smart_furi == 1,
        highlight_known_kanji: highlight == 1,
        last_synced_at: last_synced,
        known_kanji_list,
    })
}

pub fn save_wanikani_sync(
    conn: &mut Connection,
    api_token: Option<&str>,
    is_demo: bool,
    sync: &WaniKaniSyncResult,
) -> Result<WaniKaniStatus> {
    let now = Utc::now().to_rfc3339();
    let tx = conn.transaction()?;

    tx.execute(
        "UPDATE wk_profile SET api_token = ?1, is_demo = ?2, username = ?3, level = ?4, lessons_available = ?5, reviews_available = ?6, last_synced_at = ?7 WHERE id = 1",
        params![
            api_token,
            if is_demo { 1 } else { 0 },
            sync.username,
            sync.level,
            sync.lessons_available,
            sync.reviews_available,
            now,
        ],
    )?;

    tx.execute("DELETE FROM wk_subjects", [])?;
    {
        let mut ins = tx.prepare(
            "INSERT OR REPLACE INTO wk_subjects (subject_id, subject_type, characters, level, srs_stage) VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        for (sid, stype, chars, lvl, srs) in &sync.subjects {
            ins.execute(params![sid, stype, chars, lvl, srs])?;
        }
    }
    tx.commit()?;
    load_wanikani_status(conn)
}

pub fn list_grammar_summaries(
    conn: &Connection,
    jlpt_filter: Option<&str>,
    query: Option<&str>,
    status_filter: Option<&str>,
) -> Result<Vec<GrammarPointSummary>> {
    let known_kanji = load_known_kanji(conn)?;
    let mut stmt = conn.prepare(
        "SELECT g.id, g.slug, g.title, g.reading, g.meaning, g.jlpt_level, g.study_path, g.structure,
                p.srs_stage, p.next_review_at, p.ghost_level, p.correct_count, p.incorrect_count
         FROM grammar_points g
         JOIN user_progress p ON p.grammar_id = g.id
         ORDER BY CASE g.jlpt_level WHEN 'N5' THEN 1 WHEN 'N4' THEN 2 WHEN 'N3' THEN 3 WHEN 'N2' THEN 4 ELSE 5 END, g.id",
    )?;

    let mut items = Vec::new();
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, String>(6)?,
            r.get::<_, String>(7)?,
            r.get::<_, i32>(8)?,
            r.get::<_, Option<String>>(9)?,
            r.get::<_, i32>(10)?,
            r.get::<_, i32>(11)?,
            r.get::<_, i32>(12)?,
        ))
    })?;

    let q_lower = query.unwrap_or("").trim().to_lowercase();
    let jlpt = jlpt_filter.unwrap_or("ALL").to_uppercase();
    let status = status_filter.unwrap_or("all").to_lowercase();

    for row in rows {
        let (
            id,
            slug,
            title,
            reading,
            meaning,
            jlpt_level,
            study_path,
            structure,
            srs_stage,
            next_review_at,
            ghost_level,
            correct,
            incorrect,
        ) = row?;

        if jlpt != "ALL" && jlpt_level != jlpt {
            continue;
        }

        let total_attempts = correct + incorrect;
        let is_leech = incorrect >= 2 && total_attempts >= 3 && (incorrect as f32 / total_attempts as f32) >= 0.35;

        match status.as_str() {
            "learned" if srs_stage == 0 => continue,
            "new" if srs_stage > 0 => continue,
            "ghost" if ghost_level == 0 => continue,
            "leech" if !is_leech => continue,
            _ => {}
        }

        if !q_lower.is_empty() {
            let hay = format!("{} {} {} {} {}", title, reading, meaning, slug, study_path).to_lowercase();
            if !hay.contains(&q_lower) {
                continue;
            }
        }

        // Compute WaniKani kanji readiness across sentences of this grammar point
        let mut s_stmt = conn.prepare("SELECT jp_before, jp_after, answer FROM grammar_sentences WHERE grammar_id = ?1")?;
        let combined: String = s_stmt
            .query_map([id], |r| {
                Ok(format!(
                    "{}{}{}",
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(1)?
                ))
            })?
            .filter_map(|x| x.ok())
            .collect::<Vec<_>>()
            .join(" ");

        let (wk_known, wk_total, wk_pct, _) = compute_kanji_readiness(&combined, &known_kanji);

        items.push(GrammarPointSummary {
            id,
            slug,
            title,
            reading,
            meaning,
            jlpt_level,
            study_path,
            structure,
            srs_stage,
            srs_label: srs_stage_label(srs_stage),
            next_review_at,
            ghost_level,
            is_leech,
            wk_readiness_pct: wk_pct,
            wk_known_kanji: wk_known,
            wk_total_kanji: wk_total,
        });
    }

    Ok(items)
}

pub fn get_grammar_detail(conn: &Connection, grammar_id: i64) -> Result<Option<GrammarPointDetail>> {
    let known_kanji = load_known_kanji(conn)?;
    let row = conn
        .query_row(
            "SELECT g.id, g.slug, g.title, g.reading, g.meaning, g.jlpt_level, g.study_path,
                    g.structure, g.nuance, g.caution, g.confusable_with, g.pitch_note,
                    p.user_notes, p.srs_stage, p.correct_count, p.incorrect_count, p.streak, p.ghost_level, p.next_review_at
             FROM grammar_points g
             JOIN user_progress p ON p.grammar_id = g.id
             WHERE g.id = ?1",
            [grammar_id],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, String>(6)?,
                    r.get::<_, String>(7)?,
                    r.get::<_, String>(8)?,
                    r.get::<_, String>(9)?,
                    r.get::<_, String>(10)?,
                    r.get::<_, String>(11)?,
                    r.get::<_, String>(12)?,
                    r.get::<_, i32>(13)?,
                    r.get::<_, i32>(14)?,
                    r.get::<_, i32>(15)?,
                    r.get::<_, i32>(16)?,
                    r.get::<_, i32>(17)?,
                    r.get::<_, Option<String>>(18)?,
                ))
            },
        )
        .optional()?;

    let Some((
        id,
        slug,
        title,
        reading,
        meaning,
        jlpt_level,
        study_path,
        structure,
        nuance,
        caution,
        confusable_with,
        pitch_note,
        user_notes,
        srs_stage,
        correct_count,
        incorrect_count,
        streak,
        ghost_level,
        next_review_at,
    )) = row
    else {
        return Ok(None);
    };

    let mut s_stmt = conn.prepare(
        "SELECT id, grammar_id, jp_before, jp_after, answer, alt_answers, hint, english, nuance_note
         FROM grammar_sentences WHERE grammar_id = ?1 ORDER BY id",
    )?;
    let sentences: Vec<GrammarSentence> = s_stmt
        .query_map([id], |r| {
            let alt_raw: String = r.get(5)?;
            let alt_answers: Vec<String> = serde_json::from_str(&alt_raw).unwrap_or_default();
            Ok(GrammarSentence {
                id: r.get(0)?,
                grammar_id: r.get(1)?,
                jp_before: r.get(2)?,
                jp_after: r.get(3)?,
                answer: r.get(4)?,
                alt_answers,
                hint: r.get(6)?,
                english: r.get(7)?,
                nuance_note: r.get(8)?,
            })
        })?
        .filter_map(|x| x.ok())
        .collect();

    let combined: String = sentences
        .iter()
        .map(|s| format!("{}{}{}", s.jp_before, s.answer, s.jp_after))
        .collect::<Vec<_>>()
        .join(" ");
    let (wk_known, wk_total, wk_pct, _) = compute_kanji_readiness(&combined, &known_kanji);

    Ok(Some(GrammarPointDetail {
        id,
        slug,
        title,
        reading,
        meaning,
        jlpt_level,
        study_path,
        structure,
        nuance,
        caution,
        confusable_with,
        pitch_note,
        user_notes,
        srs_stage,
        srs_label: srs_stage_label(srs_stage),
        correct_count,
        incorrect_count,
        streak,
        ghost_level,
        next_review_at,
        sentences,
        wk_readiness_pct: wk_pct,
        wk_known_kanji: wk_known,
        wk_total_kanji: wk_total,
    }))
}

pub fn build_review_queue(
    conn: &Connection,
    mode: &str,
    jlpt_filter: Option<&str>,
    limit: usize,
) -> Result<Vec<ReviewQueueItem>> {
    let now_str = Utc::now().to_rfc3339();
    let jlpt = jlpt_filter.unwrap_or("ALL").to_uppercase();

    let mut items: Vec<ReviewQueueItem> = Vec::new();
    let mut seen_grammar: HashSet<i64> = HashSet::new();

    // Helper closure to load first/rotating sentence for a grammar row
    let mut push_grammar = |gid: i64,
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
                            is_ghost: bool,
                            item_mode: &str|
     -> Result<bool> {
        if items.len() >= limit {
            return Ok(false);
        }
        if jlpt != "ALL" && jlpt_level != jlpt {
            return Ok(true);
        }
        if !seen_grammar.insert(gid) {
            return Ok(true);
        }
        let mut s_stmt = conn.prepare(
            "SELECT id, jp_before, jp_after, answer, alt_answers, hint, english, nuance_note
             FROM grammar_sentences WHERE grammar_id = ?1 ORDER BY id",
        )?;
        let sentences: Vec<(i64, String, String, String, Vec<String>, String, String, String)> = s_stmt
            .query_map([gid], |r| {
                let alt_raw: String = r.get(4)?;
                let alts: Vec<String> = serde_json::from_str(&alt_raw).unwrap_or_default();
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    alts,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                ))
            })?
            .filter_map(|x| x.ok())
            .collect();

        if sentences.is_empty() {
            return Ok(true);
        }
        // Rotate sentence by SRS stage so higher reviews test alternate example sentences!
        let idx = (srs_stage.max(0) as usize) % sentences.len();
        let (sid, jp_before, jp_after, answer, alt_answers, hint, english, nuance_note) =
            sentences[idx].clone();

        items.push(ReviewQueueItem {
            grammar_id: gid,
            sentence_id: sid,
            title,
            reading,
            meaning,
            jlpt_level,
            structure,
            nuance,
            caution,
            confusable_with,
            jp_before,
            jp_after,
            answer,
            alt_answers,
            hint,
            english,
            nuance_note,
            srs_stage,
            srs_label: srs_stage_label(srs_stage),
            is_ghost,
            ghost_level,
            mode: item_mode.to_string(),
        });
        Ok(items.len() < limit)
    };

    match mode {
        "ghost" => {
            let mut stmt = conn.prepare(
                "SELECT g.id, g.title, g.reading, g.meaning, g.jlpt_level, g.structure, g.nuance, g.caution, g.confusable_with, p.srs_stage, p.ghost_level
                 FROM grammar_points g JOIN user_progress p ON p.grammar_id = g.id
                 WHERE p.ghost_level > 0
                 ORDER BY p.ghost_due_at ASC",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, String>(6)?,
                    r.get::<_, String>(7)?,
                    r.get::<_, String>(8)?,
                    r.get::<_, i32>(9)?,
                    r.get::<_, i32>(10)?,
                ))
            })?;
            for r in rows {
                let (id, t, rd, m, jl, st, nu, ca, co, srs, gl) = r?;
                if !push_grammar(id, t, rd, m, jl, st, nu, ca, co, srs, gl, true, "review")? {
                    break;
                }
            }
        }
        "lesson" => {
            let mut stmt = conn.prepare(
                "SELECT g.id, g.title, g.reading, g.meaning, g.jlpt_level, g.structure, g.nuance, g.caution, g.confusable_with, p.srs_stage, p.ghost_level
                 FROM grammar_points g JOIN user_progress p ON p.grammar_id = g.id
                 WHERE p.srs_stage = 0
                 ORDER BY CASE g.jlpt_level WHEN 'N5' THEN 1 WHEN 'N4' THEN 2 WHEN 'N3' THEN 3 WHEN 'N2' THEN 4 ELSE 5 END, g.id",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, String>(6)?,
                    r.get::<_, String>(7)?,
                    r.get::<_, String>(8)?,
                    r.get::<_, i32>(9)?,
                    r.get::<_, i32>(10)?,
                ))
            })?;
            for r in rows {
                let (id, t, rd, m, jl, st, nu, ca, co, srs, gl) = r?;
                if !push_grammar(id, t, rd, m, jl, st, nu, ca, co, srs, gl, false, "lesson")? {
                    break;
                }
            }
        }
        "cram" => {
            let mut stmt = conn.prepare(
                "SELECT g.id, g.title, g.reading, g.meaning, g.jlpt_level, g.structure, g.nuance, g.caution, g.confusable_with, p.srs_stage, p.ghost_level
                 FROM grammar_points g JOIN user_progress p ON p.grammar_id = g.id
                 ORDER BY p.ghost_level DESC, p.incorrect_count DESC, p.srs_stage DESC, g.id",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, String>(6)?,
                    r.get::<_, String>(7)?,
                    r.get::<_, String>(8)?,
                    r.get::<_, i32>(9)?,
                    r.get::<_, i32>(10)?,
                ))
            })?;
            for r in rows {
                let (id, t, rd, m, jl, st, nu, ca, co, srs, gl) = r?;
                if !push_grammar(id, t, rd, m, jl, st, nu, ca, co, srs, gl, gl > 0, "cram")? {
                    break;
                }
            }
        }
        _ => {
            // Due Ghost Reviews first, then due regular SRS reviews
            let mut g_stmt = conn.prepare(
                "SELECT g.id, g.title, g.reading, g.meaning, g.jlpt_level, g.structure, g.nuance, g.caution, g.confusable_with, p.srs_stage, p.ghost_level
                 FROM grammar_points g JOIN user_progress p ON p.grammar_id = g.id
                 WHERE p.ghost_level > 0 AND p.ghost_due_at IS NOT NULL AND p.ghost_due_at <= ?1
                 ORDER BY p.ghost_due_at ASC",
            )?;
            let g_rows = g_stmt.query_map([&now_str], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, String>(6)?,
                    r.get::<_, String>(7)?,
                    r.get::<_, String>(8)?,
                    r.get::<_, i32>(9)?,
                    r.get::<_, i32>(10)?,
                ))
            })?;
            for r in g_rows {
                let (id, t, rd, m, jl, st, nu, ca, co, srs, gl) = r?;
                if !push_grammar(id, t, rd, m, jl, st, nu, ca, co, srs, gl, true, "review")? {
                    break;
                }
            }

            let mut r_stmt = conn.prepare(
                "SELECT g.id, g.title, g.reading, g.meaning, g.jlpt_level, g.structure, g.nuance, g.caution, g.confusable_with, p.srs_stage, p.ghost_level
                 FROM grammar_points g JOIN user_progress p ON p.grammar_id = g.id
                 WHERE p.srs_stage > 0 AND p.srs_stage < 12 AND p.next_review_at IS NOT NULL AND p.next_review_at <= ?1
                 ORDER BY p.next_review_at ASC",
            )?;
            let r_rows = r_stmt.query_map([&now_str], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, String>(6)?,
                    r.get::<_, String>(7)?,
                    r.get::<_, String>(8)?,
                    r.get::<_, i32>(9)?,
                    r.get::<_, i32>(10)?,
                ))
            })?;
            for r in r_rows {
                let (id, t, rd, m, jl, st, nu, ca, co, srs, gl) = r?;
                if !push_grammar(id, t, rd, m, jl, st, nu, ca, co, srs, gl, false, "review")? {
                    break;
                }
            }
        }
    }

    Ok(items)
}

pub fn detect_grammar_in_text(conn: &Connection, text: &str) -> Result<Vec<DetectedGrammar>> {
    let plain = strip_ruby_markup(text);
    let mut stmt = conn.prepare(
        "SELECT g.id, g.title, g.meaning, g.jlpt_level, g.match_patterns, p.srs_stage
         FROM grammar_points g
         JOIN user_progress p ON p.grammar_id = g.id
         ORDER BY g.id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, i32>(5)?,
        ))
    })?;

    let mut found = Vec::new();
    for r in rows {
        let (id, title, meaning, jlpt_level, patterns_raw, srs_stage) = r?;
        let patterns: Vec<String> = serde_json::from_str(&patterns_raw).unwrap_or_default();
        if let Some(matched) = patterns.iter().find(|pat| !pat.is_empty() && plain.contains(pat.as_str())) {
            found.push(DetectedGrammar {
                grammar_id: id,
                title,
                meaning,
                jlpt_level,
                srs_stage,
                srs_label: srs_stage_label(srs_stage),
                matched_token: matched.clone(),
            });
        }
    }
    Ok(found)
}

pub fn list_reading_passages(conn: &Connection) -> Result<Vec<ReadingPassage>> {
    let known_kanji = load_known_kanji(conn)?;
    let mut stmt = conn.prepare(
        "SELECT id, title, jlpt_level, topic, content_ruby, english FROM reading_passages ORDER BY id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, String>(5)?,
        ))
    })?;

    let mut out = Vec::new();
    for r in rows {
        let (id, title, jlpt_level, topic, content_ruby, english) = r?;
        let (_, _, wk_pct, _) = compute_kanji_readiness(&content_ruby, &known_kanji);
        let detected = detect_grammar_in_text(conn, &content_ruby)?;
        out.push(ReadingPassage {
            id,
            title,
            jlpt_level,
            topic,
            content_ruby,
            english,
            wk_readiness_pct: wk_pct,
            detected_grammar: detected,
        });
    }
    Ok(out)
}

pub fn list_conjugations(conn: &Connection, form: Option<&str>) -> Result<Vec<ConjugationItem>> {
    let form_filter = form.unwrap_or("all").to_lowercase();
    let mut stmt = conn.prepare(
        "SELECT id, dictionary, reading, meaning, word_type, target_form, target_label, answer_kana, answer_kanji, rule_explanation, example_jp, example_en
         FROM conjugation_items ORDER BY id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(ConjugationItem {
            id: r.get(0)?,
            dictionary: r.get(1)?,
            reading: r.get(2)?,
            meaning: r.get(3)?,
            word_type: r.get(4)?,
            target_form: r.get(5)?,
            target_label: r.get(6)?,
            answer_kana: r.get(7)?,
            answer_kanji: r.get(8)?,
            rule_explanation: r.get(9)?,
            example_jp: r.get(10)?,
            example_en: r.get(11)?,
        })
    })?;

    let mut out = Vec::new();
    for r in rows {
        let item = r?;
        if form_filter != "all" && item.target_form != form_filter {
            continue;
        }
        out.push(item);
    }
    Ok(out)
}

pub fn compute_dashboard(conn: &Connection) -> Result<crate::models::DashboardStats> {
    let now = Utc::now();
    let now_str = now.to_rfc3339();

    let regular_due: i32 = conn.query_row(
        "SELECT COUNT(*) FROM user_progress WHERE srs_stage > 0 AND srs_stage < 12 AND next_review_at IS NOT NULL AND next_review_at <= ?1",
        [&now_str],
        |r| r.get(0),
    )?;
    let ghost_due: i32 = conn.query_row(
        "SELECT COUNT(*) FROM user_progress WHERE ghost_level > 0 AND ghost_due_at IS NOT NULL AND ghost_due_at <= ?1",
        [&now_str],
        |r| r.get(0),
    )?;
    let lessons_available: i32 = conn.query_row(
        "SELECT COUNT(*) FROM user_progress WHERE srs_stage = 0",
        [],
        |r| r.get(0),
    )?;
    let total_learned: i32 = conn.query_row(
        "SELECT COUNT(*) FROM user_progress WHERE srs_stage > 0",
        [],
        |r| r.get(0),
    )?;
    let total_grammar: i32 = conn.query_row("SELECT COUNT(*) FROM grammar_points", [], |r| r.get(0))?;
    let leech_count: i32 = conn.query_row(
        "SELECT COUNT(*) FROM user_progress WHERE incorrect_count >= 2 AND (correct_count + incorrect_count) >= 3 AND (CAST(incorrect_count AS REAL) / (correct_count + incorrect_count)) >= 0.35",
        [],
        |r| r.get(0),
    )?;

    // 7-day accuracy & today count
    let week_ago = (now - Duration::days(7)).to_rfc3339();
    let (reviews_7d, correct_7d): (i32, i32) = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(correct), 0) FROM review_logs WHERE reviewed_at >= ?1",
        [&week_ago],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let accuracy_7d = if reviews_7d > 0 {
        ((correct_7d as f64 / reviews_7d as f64) * 100.0).round() as u8
    } else {
        100
    };

    let today_start = now.date_naive().and_hms_opt(0, 0, 0).unwrap().and_utc().to_rfc3339();
    let today_reviews: i32 = conn.query_row(
        "SELECT COUNT(*) FROM review_logs WHERE reviewed_at >= ?1",
        [&today_start],
        |r| r.get(0),
    )?;

    // Consecutive daily study streak
    let mut streak_days = 0;
    for d in 0..365 {
        let day_begin = (now.date_naive() - Duration::days(d))
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .to_rfc3339();
        let day_end = (now.date_naive() - Duration::days(d - 1))
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .to_rfc3339();
        let cnt: i32 = conn.query_row(
            "SELECT COUNT(*) FROM review_logs WHERE reviewed_at >= ?1 AND reviewed_at < ?2",
            params![day_begin, day_end],
            |r| r.get(0),
        )?;
        if cnt > 0 {
            streak_days += 1;
        } else if d > 0 {
            break;
        }
    }

    // JLPT progress
    let mut jlpt_progress = Vec::new();
    for lvl in ["N5", "N4", "N3", "N2", "N1"] {
        let (total, learned, mastered): (i32, i32, i32) = conn.query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(CASE WHEN p.srs_stage > 0 THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN p.srs_stage >= 7 THEN 1 ELSE 0 END), 0)
             FROM grammar_points g JOIN user_progress p ON p.grammar_id = g.id
             WHERE g.jlpt_level = ?1",
            [lvl],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        let percent = if total > 0 {
            ((learned as f64 / total as f64) * 100.0).round() as u8
        } else {
            0
        };
        jlpt_progress.push(JlptProgress {
            level: lvl.to_string(),
            learned,
            total,
            mastered,
            percent,
        });
    }

    // 24h forecast buckets
    let mut due_times: Vec<DateTime<Utc>> = Vec::new();
    {
        let mut stmt = conn.prepare(
            "SELECT next_review_at, ghost_due_at FROM user_progress WHERE srs_stage > 0 AND srs_stage < 12",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, Option<String>>(0)?, r.get::<_, Option<String>>(1)?))
        })?;
        for r in rows {
            let (nr, gd) = r?;
            if let Some(s) = nr {
                if let Ok(dt) = DateTime::parse_from_rfc3339(&s) {
                    due_times.push(dt.with_timezone(&Utc));
                }
            }
            if let Some(s) = gd {
                if let Ok(dt) = DateTime::parse_from_rfc3339(&s) {
                    due_times.push(dt.with_timezone(&Utc));
                }
            }
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
    let mut forecast_24h = Vec::new();
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
    let tiers: [(&str, &str, i32, i32); 5] = [
        ("Apprentice", "1–4", 1, 4),
        ("Guru", "5–6", 5, 6),
        ("Master", "7–8", 7, 8),
        ("Enlightened", "9–10", 9, 10),
        ("Burned", "11–12", 11, 12),
    ];
    let mut srs_breakdown = Vec::new();
    for (tier, range, lo, hi) in tiers {
        let count: i32 = conn.query_row(
            "SELECT COUNT(*) FROM user_progress WHERE srs_stage >= ?1 AND srs_stage <= ?2",
            params![lo, hi],
            |r| r.get(0),
        )?;
        srs_breakdown.push(SrsTierCount {
            tier: tier.to_string(),
            range: range.to_string(),
            count,
        });
    }

    let wanikani = load_wanikani_status(conn)?;

    Ok(crate::models::DashboardStats {
        reviews_due: regular_due + ghost_due,
        ghost_reviews_due: ghost_due,
        lessons_available,
        total_learned,
        total_grammar,
        accuracy_7d,
        reviews_7d,
        streak_days,
        daily_goal: 20,
        today_reviews,
        leech_count,
        jlpt_progress,
        forecast_24h,
        srs_breakdown,
        wanikani,
    })
}

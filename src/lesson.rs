//! Rich grammar-lesson model, embedded content loader, and validators.
//!
//! Lessons live as one JSON file per slug under `content/lessons/` and are
//! stored in `grammar_points.lesson` (JSONB). Points without a file keep the
//! existing short fields (`nuance`, `caution`, `confusable_with`) as fallback.

use anyhow::{Context, Result, bail};
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(RustEmbed)]
#[folder = "content/lessons/"]
struct LessonAssets;

/// Full lesson document for one grammar point.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GrammarLesson {
    /// One-line gloss(es); may enrich the point's short `meaning`.
    pub meaning: String,
    /// Plain-English mental model.
    pub gist: String,
    pub formation: FormationTable,
    /// 2–3 short paragraphs.
    pub explanation: String,
    pub senses: Vec<LessonSense>,
    pub register_context: String,
    pub mistakes: Vec<LessonMistake>,
    pub contrasts: Vec<LessonContrast>,
    /// Worked full-sentence examples (answers visible in Learn New).
    pub examples: Vec<LessonExample>,
    pub dialogue: Vec<DialogueLine>,
    pub checks: Vec<LessonCheck>,
    pub related: LessonRelated,
    #[serde(default = "default_lesson_version")]
    pub lesson_version: i32,
}

fn default_lesson_version() -> i32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FormationTable {
    pub columns: Vec<String>,
    /// Each row is a list of cell strings (ruby markup allowed); length must match `columns`.
    pub rows: Vec<Vec<String>>,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LessonSense {
    pub label: String,
    #[serde(default)]
    pub body: String,
    pub example_ruby: String,
    pub example_en: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LessonMistake {
    pub wrong_ruby: String,
    pub right_ruby: String,
    pub why: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LessonContrast {
    /// Optional slug of another point in the app (used for reciprocal checks).
    #[serde(default)]
    pub other_slug: Option<String>,
    pub label: String,
    pub summary: String,
    pub ex_a_ruby: String,
    pub ex_a_en: String,
    pub ex_b_ruby: String,
    pub ex_b_en: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LessonExample {
    pub ruby: String,
    pub english: String,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DialogueLine {
    pub speaker: String,
    pub ruby: String,
    pub english: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LessonCheck {
    pub prompt: String,
    pub options: Vec<String>,
    pub answer_idx: usize,
    #[serde(default)]
    pub why: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LessonRelated {
    #[serde(default)]
    pub prerequisites: Vec<String>,
    #[serde(default)]
    pub next: Vec<String>,
}

/// Validate furigana markup: every `[…]` must be `[base|reading]` with non-empty parts.
pub fn validate_furigana(text: &str, field: &str) -> Result<()> {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'[' {
            let rest = &text[i..];
            let Some(close_rel) = rest.find(']') else {
                bail!("{field}: unclosed '[' in furigana markup");
            };
            let inner = &rest[1..close_rel];
            let Some((base, reading)) = inner.split_once('|') else {
                bail!("{field}: furigana segment missing '|' : [{inner}]");
            };
            if base.is_empty() || reading.is_empty() {
                bail!("{field}: empty base or reading in [{inner}]");
            }
            if base.contains('[') || reading.contains('[') || reading.contains(']') {
                bail!("{field}: nested or malformed furigana in [{inner}]");
            }
            i += close_rel + 1;
            continue;
        }
        i += 1;
    }
    Ok(())
}

fn validate_text_fields(slug: &str, lesson: &GrammarLesson) -> Result<()> {
    validate_furigana(&lesson.gist, &format!("lesson '{slug}' gist"))?;
    validate_furigana(&lesson.explanation, &format!("lesson '{slug}' explanation"))?;
    validate_furigana(
        &lesson.register_context,
        &format!("lesson '{slug}' register_context"),
    )?;
    if !lesson.formation.note.is_empty() {
        validate_furigana(
            &lesson.formation.note,
            &format!("lesson '{slug}' formation.note"),
        )?;
    }
    for (i, sense) in lesson.senses.iter().enumerate() {
        validate_furigana(
            &sense.example_ruby,
            &format!("lesson '{slug}' sense[{i}].example_ruby"),
        )?;
        if !sense.body.is_empty() {
            validate_furigana(&sense.body, &format!("lesson '{slug}' sense[{i}].body"))?;
        }
    }
    for (i, m) in lesson.mistakes.iter().enumerate() {
        validate_furigana(
            &m.wrong_ruby,
            &format!("lesson '{slug}' mistake[{i}].wrong_ruby"),
        )?;
        validate_furigana(
            &m.right_ruby,
            &format!("lesson '{slug}' mistake[{i}].right_ruby"),
        )?;
    }
    for (i, c) in lesson.contrasts.iter().enumerate() {
        validate_furigana(
            &c.ex_a_ruby,
            &format!("lesson '{slug}' contrast[{i}].ex_a_ruby"),
        )?;
        validate_furigana(
            &c.ex_b_ruby,
            &format!("lesson '{slug}' contrast[{i}].ex_b_ruby"),
        )?;
    }
    for (i, e) in lesson.examples.iter().enumerate() {
        validate_furigana(&e.ruby, &format!("lesson '{slug}' example[{i}].ruby"))?;
    }
    for (i, d) in lesson.dialogue.iter().enumerate() {
        validate_furigana(&d.ruby, &format!("lesson '{slug}' dialogue[{i}].ruby"))?;
    }
    Ok(())
}

/// Structural + furigana validation for a single lesson (slug used in error messages).
pub fn validate_lesson(slug: &str, lesson: &GrammarLesson) -> Result<()> {
    if lesson.gist.trim().is_empty() {
        bail!("lesson '{slug}': gist is required");
    }
    if lesson.explanation.trim().is_empty() {
        bail!("lesson '{slug}': explanation is required");
    }
    if lesson.formation.columns.is_empty() {
        bail!("lesson '{slug}': formation.columns must not be empty");
    }
    if lesson.formation.rows.is_empty() {
        bail!("lesson '{slug}': formation.rows must not be empty");
    }
    let ncols = lesson.formation.columns.len();
    for (i, row) in lesson.formation.rows.iter().enumerate() {
        if row.len() != ncols {
            bail!(
                "lesson '{slug}': formation row {i} has {} cells, expected {ncols}",
                row.len()
            );
        }
        for (j, cell) in row.iter().enumerate() {
            validate_furigana(cell, &format!("lesson '{slug}' formation[{i}][{j}]"))?;
        }
    }
    if lesson.senses.is_empty() {
        bail!("lesson '{slug}': at least one sense is required");
    }
    if lesson.mistakes.is_empty() {
        bail!("lesson '{slug}': at least one common mistake is required");
    }
    if lesson.contrasts.is_empty() {
        bail!("lesson '{slug}': at least one contrast is required");
    }
    if lesson.examples.is_empty() {
        bail!("lesson '{slug}': at least one example is required");
    }
    if lesson.checks.is_empty() {
        bail!("lesson '{slug}': at least one quick check is required");
    }
    for (i, check) in lesson.checks.iter().enumerate() {
        if check.options.len() < 2 {
            bail!("lesson '{slug}': check[{i}] needs at least 2 options");
        }
        if check.answer_idx >= check.options.len() {
            bail!("lesson '{slug}': check[{i}] answer_idx out of range");
        }
    }
    validate_text_fields(slug, lesson)?;
    for (i, line) in lesson.dialogue.iter().enumerate() {
        if line.speaker.trim().is_empty() {
            bail!("lesson '{slug}': dialogue[{i}] speaker is empty");
        }
    }
    Ok(())
}

/// Parse and validate a lesson JSON document.
pub fn parse_lesson_json(slug: &str, raw: &str) -> Result<GrammarLesson> {
    let lesson: GrammarLesson = serde_json::from_str(raw)
        .with_context(|| format!("failed to parse lesson JSON for '{slug}'"))?;
    validate_lesson(slug, &lesson)?;
    Ok(lesson)
}

/// Load all embedded lesson files keyed by slug (filename without `.json`).
pub fn load_embedded_lessons() -> Result<HashMap<String, GrammarLesson>> {
    let mut map = HashMap::new();
    for path in LessonAssets::iter() {
        let path = path.as_ref();
        if !path.ends_with(".json") {
            continue;
        }
        let slug = path
            .trim_end_matches(".json")
            .rsplit('/')
            .next()
            .unwrap_or(path)
            .to_string();
        let file = LessonAssets::get(path)
            .with_context(|| format!("missing embedded lesson asset '{path}'"))?;
        let raw = std::str::from_utf8(file.data.as_ref())
            .with_context(|| format!("lesson '{slug}' is not valid UTF-8"))?;
        let lesson = parse_lesson_json(&slug, raw)?;
        map.insert(slug, lesson);
    }
    validate_reciprocal_contrasts(&map)?;
    Ok(map)
}

/// When both sides have lessons and name each other via `other_slug`, the link must be mutual.
pub fn validate_reciprocal_contrasts(lessons: &HashMap<String, GrammarLesson>) -> Result<()> {
    for (slug, lesson) in lessons {
        for contrast in &lesson.contrasts {
            let Some(other) = contrast.other_slug.as_deref() else {
                continue;
            };
            if other == slug {
                bail!("lesson '{slug}': contrast other_slug must not be self");
            }
            let Some(other_lesson) = lessons.get(other) else {
                // Other point may not have a rich lesson yet — allowed.
                continue;
            };
            let back = other_lesson
                .contrasts
                .iter()
                .any(|c| c.other_slug.as_deref() == Some(slug.as_str()));
            if !back {
                bail!(
                    "lesson '{slug}' contrasts with '{other}', but '{other}' does not list '{slug}'"
                );
            }
        }
    }
    Ok(())
}

/// Known curriculum slugs (for optional soft checks in tests).
pub fn assert_lesson_slugs_exist(
    lessons: &HashMap<String, GrammarLesson>,
    known_slugs: &HashSet<&str>,
) -> Result<()> {
    for slug in lessons.keys() {
        if !known_slugs.contains(slug.as_str()) {
            bail!("lesson file '{slug}.json' has no matching grammar point slug");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_lesson() -> GrammarLesson {
        GrammarLesson {
            meaning: "test".into(),
            gist: "gist text".into(),
            formation: FormationTable {
                columns: vec!["Host".into(), "Form".into(), "Example".into()],
                rows: vec![vec![
                    "V".into(),
                    "ている".into(),
                    "[食|た]べている".into(),
                ]],
                note: String::new(),
            },
            explanation: "explanation paragraph".into(),
            senses: vec![LessonSense {
                label: "progress".into(),
                body: String::new(),
                example_ruby: "[雨|あめ]が[降|ふ]っている。".into(),
                example_en: "It is raining.".into(),
            }],
            register_context: "neutral".into(),
            mistakes: vec![LessonMistake {
                wrong_ruby: "[知|し]っていません。".into(),
                right_ruby: "[知|し]りません。".into(),
                why: "negative of 知っている is 知らない".into(),
            }],
            contrasts: vec![LessonContrast {
                other_slug: None,
                label: "ている vs た".into(),
                summary: "state vs event".into(),
                ex_a_ruby: "[結婚|けっこん]した。".into(),
                ex_a_en: "got married".into(),
                ex_b_ruby: "[結婚|けっこん]している。".into(),
                ex_b_en: "is married".into(),
            }],
            examples: vec![LessonExample {
                ruby: "[今|いま]、[雨|あめ]が[降|ふ]っています。".into(),
                english: "It's raining now.".into(),
                note: String::new(),
            }],
            dialogue: vec![DialogueLine {
                speaker: "A".into(),
                ruby: "[今|いま]、[何|なに]をしていますか。".into(),
                english: "What are you doing now?".into(),
            }],
            checks: vec![LessonCheck {
                prompt: "窓が（　）います。".into(),
                options: vec!["開いて".into(), "開けて".into()],
                answer_idx: 0,
                why: "intransitive".into(),
            }],
            related: LessonRelated {
                prerequisites: vec![],
                next: vec![],
            },
            lesson_version: 1,
        }
    }

    #[test]
    fn furigana_ok_and_bad() {
        validate_furigana("[食|た]べる", "t").unwrap();
        assert!(validate_furigana("[食べる]", "t").is_err());
        assert!(validate_furigana("[食|]", "t").is_err());
        assert!(validate_furigana("[食|た", "t").is_err());
    }

    #[test]
    fn validate_minimal_lesson() {
        validate_lesson("te-iru", &minimal_lesson()).unwrap();
    }

    #[test]
    fn rejects_bad_formation_width() {
        let mut lesson = minimal_lesson();
        lesson.formation.rows[0].pop();
        assert!(validate_lesson("te-iru", &lesson).is_err());
    }

    #[test]
    fn reciprocal_contrasts_enforced_when_both_exist() {
        let mut a = minimal_lesson();
        a.contrasts[0].other_slug = Some("b".into());
        let mut b = minimal_lesson();
        b.contrasts[0].other_slug = Some("a".into());
        let mut map = HashMap::new();
        map.insert("a".into(), a.clone());
        map.insert("b".into(), b);
        validate_reciprocal_contrasts(&map).unwrap();

        let mut map2 = HashMap::new();
        map2.insert("a".into(), a);
        map2.insert("b".into(), minimal_lesson());
        assert!(validate_reciprocal_contrasts(&map2).is_err());
    }

    #[test]
    fn embedded_sample_lessons_load_and_validate() {
        let lessons = load_embedded_lessons().expect("load lessons");
        for slug in [
            "te-iru",
            "koto-ni-suru",
            "te-mo-ii",
            "te-wa-ikenai",
            "nakereba-naranai",
            "nakute-mo-ii",
            "tai",
            "ta-koto-ga-aru",
            "kara-reason",
            "node",
            "tsumori",
            "hou-ga-ii",
            "nagara",
            "no-ga-jouzu",
            "koto-ga-dekiru",
            "to-omou",
            "mae-ni",
            "ato-de",
            "shi-listing",
            "tara-conditional",
            "ba-conditional",
            "nara-conditional",
            "kamo-shirenai",
            "sou-da-hearsay",
            "you-da",
            "noni",
            "hazu-da",
        ] {
            assert!(
                lessons.contains_key(slug),
                "missing embedded lesson '{slug}'"
            );
        }
        assert_eq!(lessons.len(), 27);
    }
}

use chrono::{DateTime, Duration, Utc};

/// Bunpro-inspired 12-stage SRS intervals.
/// Stage 0 = New/Unlearned, Stage 12 = Burned (Mastered).
pub fn srs_interval_hours(stage: i32) -> Option<i64> {
    match stage {
        1 => Some(4),     // Apprentice I: 4h
        2 => Some(8),     // Apprentice II: 8h
        3 => Some(24),    // Apprentice III: 1d
        4 => Some(48),    // Apprentice IV: 2d
        5 => Some(96),    // Guru I: 4d
        6 => Some(192),   // Guru II: 8d
        7 => Some(336),   // Master I: 14d
        8 => Some(720),   // Master II: 30d
        9 => Some(1440),  // Enlightened I: 60d
        10 => Some(2880), // Enlightened II: 120d
        11 => Some(4320), // Seasoned: 180d
        _ => None,        // Stage 0 (New) or 12+ (Burned)
    }
}

pub fn srs_stage_label(stage: i32) -> String {
    match stage {
        0 => "New Lesson".to_string(),
        1 => "Apprentice I".to_string(),
        2 => "Apprentice II".to_string(),
        3 => "Apprentice III".to_string(),
        4 => "Apprentice IV".to_string(),
        5 => "Guru I".to_string(),
        6 => "Guru II".to_string(),
        7 => "Master I".to_string(),
        8 => "Master II".to_string(),
        9 => "Enlightened I".to_string(),
        10 => "Enlightened II".to_string(),
        11 => "Seasoned".to_string(),
        _ => "Burned".to_string(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GhostIntensity {
    Minimal,
    Standard,
    Heavy,
}

impl GhostIntensity {
    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "minimal" | "low" => Self::Minimal,
            "heavy" | "high" => Self::Heavy,
            _ => Self::Standard,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Minimal => "minimal",
            Self::Standard => "standard",
            Self::Heavy => "heavy",
        }
    }

    pub fn spawn_level(self) -> i32 {
        match self {
            Self::Minimal => 1,
            Self::Standard => 3,
            Self::Heavy => 4,
        }
    }
}

pub struct SrsTransition {
    pub new_stage: i32,
    pub next_review_at: Option<DateTime<Utc>>,
    pub new_ghost_level: i32,
    pub ghost_due_at: Option<DateTime<Utc>>,
}

/// Computes next SRS stage & Ghost Review scheduling using Standard intensity.
pub fn compute_srs_transition(
    current_stage: i32,
    current_ghost_level: i32,
    is_ghost_review: bool,
    correct: bool,
    now: DateTime<Utc>,
) -> SrsTransition {
    compute_srs_transition_with_intensity(
        current_stage,
        current_ghost_level,
        is_ghost_review,
        correct,
        GhostIntensity::Standard,
        now,
    )
}

/// Computes next SRS stage & Ghost Review scheduling respecting the user's `GhostIntensity`.
pub fn compute_srs_transition_with_intensity(
    current_stage: i32,
    current_ghost_level: i32,
    is_ghost_review: bool,
    correct: bool,
    intensity: GhostIntensity,
    now: DateTime<Utc>,
) -> SrsTransition {
    if is_ghost_review {
        if correct {
            let next_ghost = (current_ghost_level - 1).max(0);
            let ghost_due = match next_ghost {
                3 => Some(now + Duration::hours(6)),
                2 => Some(now + Duration::hours(12)),
                1 => Some(now + Duration::hours(24)),
                _ => None,
            };
            SrsTransition {
                new_stage: current_stage,
                next_review_at: None, // keep existing main SRS schedule
                new_ghost_level: next_ghost,
                ghost_due_at: ghost_due,
            }
        } else {
            let reset_level = intensity.spawn_level();
            let delay_hours = match intensity {
                GhostIntensity::Heavy => 2,
                GhostIntensity::Standard => 4,
                GhostIntensity::Minimal => 12,
            };
            SrsTransition {
                new_stage: current_stage,
                next_review_at: None,
                new_ghost_level: reset_level,
                ghost_due_at: Some(now + Duration::hours(delay_hours)),
            }
        }
    } else if correct {
        let new_stage = (current_stage + 1).clamp(1, 12);
        let next_review = srs_interval_hours(new_stage).map(|h| now + Duration::hours(h));
        SrsTransition {
            new_stage,
            next_review_at: next_review,
            new_ghost_level: current_ghost_level,
            ghost_due_at: None,
        }
    } else {
        // Demote 1 stage for Apprentice, 2 stages for Guru+
        let penalty = if current_stage >= 5 { 2 } else { 1 };
        let new_stage = (current_stage - penalty).max(1);
        let next_review = srs_interval_hours(new_stage).map(|h| now + Duration::hours(h));

        let spawn_level = intensity.spawn_level();
        let ghost_delay_hours = match intensity {
            GhostIntensity::Heavy => 2,
            GhostIntensity::Standard => 4,
            GhostIntensity::Minimal => 12,
        };
        SrsTransition {
            new_stage,
            next_review_at: next_review,
            new_ghost_level: spawn_level,
            ghost_due_at: Some(now + Duration::hours(ghost_delay_hours)),
        }
    }
}

/// Normalize Japanese answer text for forgiving yet accurate grading.
pub fn normalize_jp(input: &str) -> String {
    input
        .trim()
        .chars()
        .filter(|c| {
            !c.is_whitespace()
                && !matches!(
                    c,
                    '。' | '、' | '！' | '？' | '.' | ',' | '!' | '?' | '〜' | '～' | '「' | '」'
                )
        })
        .map(|c| {
            // Convert Katakana to Hiragana for forgiving kana comparison
            let code = c as u32;
            if (0x30A1..=0x30F6).contains(&code) {
                char::from_u32(code - 0x60).unwrap_or(c)
            } else {
                c
            }
        })
        .collect::<String>()
        .to_lowercase()
}

pub fn check_answer(user_input: &str, primary: &str, alts: &[String]) -> Option<String> {
    let norm_in = normalize_jp(user_input);
    if norm_in.is_empty() {
        return None;
    }
    if norm_in == normalize_jp(primary) {
        return Some(primary.to_string());
    }
    for alt in alts {
        if norm_in == normalize_jp(alt) {
            return Some(alt.clone());
        }
    }
    None
}

/// Rich grading outcome supporting Bunpro-style non-penalizing "almost right" shake hints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GradeOutcome {
    Correct { matched: String },
    AlmostRight { hint: String, category: &'static str },
    Incorrect,
}

const REGISTER_PAIRS: &[(&str, &str)] = &[
    ("なければなりません", "なければならない"),
    ("なければなりませんでした", "なければならなかった"),
    ("なくてはなりません", "なくてはいけない"),
    ("てはいけません", "てはいけない"),
    ("ではいけません", "ではいけない"),
    ("てはいけませんでした", "てはいけなかった"),
    ("なくてもいいです", "なくてもいい"),
    ("なくてもよかったです", "なくてもよかった"),
    ("ことができます", "ことができる"),
    ("ことができません", "ことができない"),
    ("ことができました", "ことができた"),
    ("ことができませんでした", "ことができなかった"),
    ("と思います", "と思う"),
    ("と思っています", "と思っている"),
    ("と思いました", "と思った"),
    ("かもしれません", "かもしれない"),
    ("わけではありません", "わけではない"),
    ("わけにはいきません", "わけにはいかない"),
    ("に違いありません", "に違いない"),
    ("にちがいありません", "にちがいない"),
    ("ざるを得ません", "ざるを得ない"),
    ("ざるを得ませんでした", "ざるを得なかった"),
    ("ていました", "ていた"),
    ("ていません", "ていない"),
    ("ています", "ている"),
    ("ています", "てる"),
    ("でいます", "でいる"),
    ("ておきます", "ておく"),
    ("ておきました", "ておいた"),
    ("てしまいます", "てしまう"),
    ("てしまいました", "てしまった"),
    ("でしまいました", "でしまった"),
    ("てみます", "てみる"),
    ("てみました", "てみた"),
    ("てくれます", "てくれる"),
    ("てくれました", "てくれた"),
    ("てくれませんでした", "てくれなかった"),
    ("ようにしています", "ようにしている"),
    ("ようにします", "ようにする"),
    ("ようになりました", "ようになった"),
    ("ことにしました", "ことにした"),
    ("ことにしています", "ことにしている"),
    ("ことにします", "ことにする"),
    ("ことになりました", "ことになった"),
    ("ことになっています", "ことになっている"),
    ("ことになります", "ことになる"),
    ("ことがあります", "ことがある"),
    ("ことがありました", "ことがあった"),
    ("たことがあります", "たことがある"),
    ("たことがありません", "たことがない"),
    ("ことはありません", "ことはない"),
    ("たほうがいいです", "たほうがいい"),
    ("ないほうがいいです", "ないほうがいい"),
    ("つもりです", "つもりだ"),
    ("つもりでした", "つもりだった"),
    ("はずです", "はずだ"),
    ("はずでした", "はずだった"),
    ("はずがありません", "はずがない"),
    ("そうです", "そうだ"),
    ("ようです", "ようだ"),
    ("べきです", "べきだ"),
    ("べきではありません", "べきではない"),
    ("べきでした", "べきだった"),
    ("ということです", "ということだ"),
    ("一方です", "一方だ"),
    ("やすいです", "やすい"),
    ("にくいです", "にくい"),
    ("たいです", "たい"),
    ("たかったです", "たかった"),
    ("おかげです", "おかげだ"),
    ("せいです", "せいだ"),
    ("ました", "た"),
    ("ませんでした", "なかった"),
    ("ません", "ない"),
    ("でした", "だった"),
    ("です", "だ"),
];

const TENSE_PAIRS: &[(&str, &str)] = &[
    ("ています", "ていました"),
    ("ている", "ていた"),
    ("てる", "てた"),
    ("なければなりません", "なければなりませんでした"),
    ("なければならない", "なければならなかった"),
    ("なくてもいいです", "なくてもよかったです"),
    ("なくてもいい", "なくてもよかった"),
    ("ことができます", "ことができました"),
    ("ことができる", "ことができた"),
    ("ことができません", "ことができませんでした"),
    ("ことができない", "ことができなかった"),
    ("と思います", "と思いました"),
    ("と思う", "と思った"),
    ("ておきます", "ておきました"),
    ("ておく", "ておいた"),
    ("てしまいます", "てしまいました"),
    ("てしまう", "てしまった"),
    ("てみます", "てみました"),
    ("てみる", "てみた"),
    ("てくれます", "てくれました"),
    ("てくれる", "てくれた"),
    ("ようにします", "ようにしました"),
    ("ようにする", "ようにした"),
    ("ことにします", "ことにしました"),
    ("ことにする", "ことにした"),
    ("ことになります", "ことになりました"),
    ("ことになる", "ことになった"),
    ("ことがあります", "ことがありました"),
    ("ことがある", "ことがあった"),
    ("つもりです", "つもりでした"),
    ("つもりだ", "つもりだった"),
    ("はずです", "はずでした"),
    ("はずだ", "はずだった"),
    ("べきです", "べきでした"),
    ("べきだ", "べきだった"),
    ("ざるを得ない", "ざるを得なかった"),
    ("ざるを得ません", "ざるを得ませんでした"),
    ("たいです", "たかったです"),
    ("たい", "たかった"),
    ("やすい", "やすかった"),
    ("にくい", "にくかった"),
    ("ません", "ませんでした"),
    ("ます", "ました"),
    ("です", "でした"),
    ("だ", "だった"),
];

const POLARITY_PAIRS: &[(&str, &str)] = &[
    ("ています", "ていません"),
    ("ている", "ていない"),
    ("ことができます", "ことができません"),
    ("ことができる", "ことができない"),
    ("たことがあります", "たことがありません"),
    ("たことがある", "たことがない"),
    ("たほうがいいです", "ないほうがいいです"),
    ("たほうがいい", "ないほうがいい"),
    ("べきです", "べきではありません"),
    ("べきだ", "べきではない"),
    ("はずです", "はずがありません"),
    ("はずだ", "はずがない"),
    ("てくれました", "てくれませんでした"),
    ("てくれた", "てくれなかった"),
    ("たいです", "たくないです"),
    ("たい", "たくない"),
    ("やすいです", "やすくないです"),
    ("やすい", "やすくない"),
    ("ます", "ません"),
    ("ました", "ませんでした"),
];

const CONFUSABLE_GRAMMAR: &[(&str, &str, &str)] = &[
    (
        "のに",
        "ても",
        "「ても」 is hypothetical ('even if'), whereas this sentence contrasts an actual fact — try 「のに」 or check the nuance!",
    ),
    (
        "のに",
        "けど",
        "「けど」 is a soft neutral 'but'; this sentence expects a stronger unexpected contrast.",
    ),
    (
        "のに",
        "にもかかわらず",
        "Close! Check the formality level between 「のに」 and 「にもかかわらず」.",
    ),
    (
        "のに",
        "ものの",
        "Close! 「ものの」 is a calm formal concession, while 「のに」 carries stronger surprise/contrast.",
    ),
    (
        "ために",
        "ように",
        "Close! 「ために」 pairs with volitional goals/causes, while 「ように」 pairs with potential/non-volitional outcomes.",
    ),
    (
        "おかげで",
        "せいで",
        "Opposite polarity! 「おかげで」 is for positive gratitude, while 「せいで」 blames a negative result.",
    ),
    (
        "から",
        "ので",
        "Close! Both mean 'because' — check whether the sentence wants 「から」 or softer 「ので」.",
    ),
    (
        "たら",
        "ば",
        "Close conditional! Check the conjugation/nuance between 「たら」 and 「ば」.",
    ),
    (
        "たら",
        "なら",
        "Close conditional! 「なら」 picks up a contextual assumption, whereas 「たら」 focuses on completion.",
    ),
    (
        "ば",
        "なら",
        "Close conditional! Check whether this context calls for 「ば」 or 「なら」.",
    ),
    (
        "てはいけない",
        "なくてもいい",
        "Watch the meaning: 「てはいけない」 = must NOT, while 「なくてもいい」 = don't have to!",
    ),
    (
        "ことにする",
        "ことになる",
        "Close! 「ことにする」 is a personal decision, while 「ことになる」 is an external/scheduled decision.",
    ),
    (
        "ことにした",
        "ことになった",
        "Close! 「ことにした」 = I decided, while 「ことになった」 = it was decided externally.",
    ),
    (
        "ことにしました",
        "ことになりました",
        "Close! 「ことにしました」 = I decided, while 「ことになりました」 = it was decided externally.",
    ),
    (
        "そうだ",
        "ようだ",
        "Close conjecture! 「そうだ」 (hearsay) vs 「ようだ」 (direct sensory impression).",
    ),
    (
        "そうです",
        "ようです",
        "Close conjecture! 「そうです」 (hearsay) vs 「ようです」 (direct impression).",
    ),
    (
        "てみる",
        "ておく",
        "Close auxiliary verb! 「てみる」 = try doing, while 「ておく」 = do in advance.",
    ),
    (
        "ておく",
        "てしまう",
        "Close auxiliary verb! 「ておく」 = do in advance, while 「てしまう」 = finish completely / regret.",
    ),
    (
        "について",
        "に対して",
        "Close! 「について」 = about a topic, while 「に対して」 = directed toward / in contrast to.",
    ),
    (
        "について",
        "にとって",
        "Close! 「について」 = about a topic, while 「にとって」 = from someone's standpoint.",
    ),
    (
        "に対して",
        "にとって",
        "Close! 「に対して」 = toward/against, while 「にとって」 = for someone's perspective.",
    ),
    (
        "前に",
        "あとで",
        "Opposite timeline! 「前に」 = before (dictionary form), 「あとで」 = after (た-form).",
    ),
    (
        "わけではない",
        "わけにはいかない",
        "Close! 「わけではない」 = it's not that..., while 「わけにはいかない」 = cannot afford to...",
    ),
    (
        "わけではありません",
        "わけにはいきません",
        "Close! 「わけではありません」 = it's not that..., while 「わけにはいきません」 = cannot afford to...",
    ),
    (
        "ばかり",
        "だけ",
        "Close! 「ばかり」 implies 'nothing but / just did', distinct from neutral 「だけ」.",
    ),
    (
        "うちに",
        "間に",
        "Close! 「うちに」 emphasizes acting before a state changes.",
    ),
    (
        "にかかわらず",
        "にもかかわらず",
        "Watch the も! 「にもかかわらず」 = despite, while 「にかかわらず」 = regardless of.",
    ),
];

fn matches_any_target(candidate: &str, targets: &[String]) -> bool {
    let norm = normalize_jp(candidate);
    targets.contains(&norm)
}

fn try_pair_transform(norm_in: &str, targets: &[String], pairs: &[(&str, &str)]) -> Option<bool> {
    for &(a, b) in pairs {
        let na = normalize_jp(a);
        let nb = normalize_jp(b);
        if norm_in == na && targets.contains(&nb) {
            return Some(true); // user gave `a`, target wanted `b`
        }
        if norm_in == nb && targets.contains(&na) {
            return Some(false); // user gave `b`, target wanted `a`
        }
        if let Some(prefix) = norm_in.strip_suffix(&na) {
            let swapped = format!("{}{}", prefix, nb);
            if matches_any_target(&swapped, targets) {
                return Some(true);
            }
        }
        if let Some(prefix) = norm_in.strip_suffix(&nb) {
            let swapped = format!("{}{}", prefix, na);
            if matches_any_target(&swapped, targets) {
                return Some(false);
            }
        }
    }
    None
}

fn levenshtein_chars(a: &str, b: &str) -> usize {
    let av: Vec<char> = a.chars().collect();
    let bv: Vec<char> = b.chars().collect();
    if av.is_empty() {
        return bv.len();
    }
    if bv.is_empty() {
        return av.len();
    }
    let mut prev: Vec<usize> = (0..=bv.len()).collect();
    let mut curr = vec![0usize; bv.len() + 1];
    for (i, &ca) in av.iter().enumerate() {
        curr[0] = i + 1;
        for (j, &cb) in bv.iter().enumerate() {
            let cost = usize::from(ca != cb);
            curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[bv.len()]
}

/// Grades a learner's answer with contextual nuance detection (politeness/register, tense,
/// polarity, confusable grammar synonyms, and 1-character kana typos).
pub fn grade_answer(
    user_input: &str,
    primary: &str,
    alts: &[String],
    hint: &str,
) -> GradeOutcome {
    if let Some(matched) = check_answer(user_input, primary, alts) {
        return GradeOutcome::Correct { matched };
    }

    let norm_in = normalize_jp(user_input);
    if norm_in.is_empty() {
        return GradeOutcome::Incorrect;
    }

    let mut targets = Vec::with_capacity(1 + alts.len());
    targets.push(normalize_jp(primary));
    for alt in alts {
        let n = normalize_jp(alt);
        if !n.is_empty() && !targets.contains(&n) {
            targets.push(n);
        }
    }

    // 1. Politeness / Register mismatch
    if let Some(user_was_polite) = try_pair_transform(&norm_in, &targets, REGISTER_PAIRS) {
        let msg = if user_was_polite || hint.to_lowercase().contains("casual") {
            "Almost! Right grammar, wrong register — try the casual / plain form."
        } else {
            "Almost! Right grammar, wrong register — try the polite (〜ます / 〜です) form."
        };
        return GradeOutcome::AlmostRight {
            hint: msg.to_string(),
            category: "register",
        };
    }

    // 2. Tense mismatch (present <-> past)
    if let Some(user_was_present) = try_pair_transform(&norm_in, &targets, TENSE_PAIRS) {
        let msg = if user_was_present {
            "Almost! Check the tense — this sentence needs the past form."
        } else {
            "Almost! Check the tense — this sentence needs the non-past (present/future) form."
        };
        return GradeOutcome::AlmostRight {
            hint: msg.to_string(),
            category: "tense",
        };
    }

    // 3. Polarity mismatch (affirmative <-> negative)
    if let Some(user_was_affirmative) = try_pair_transform(&norm_in, &targets, POLARITY_PAIRS) {
        let msg = if user_was_affirmative {
            "Almost! Check polarity — this sentence needs the negative form."
        } else {
            "Almost! Check polarity — this sentence needs the affirmative form."
        };
        return GradeOutcome::AlmostRight {
            hint: msg.to_string(),
            category: "polarity",
        };
    }

    // 4. Close synonym / confusable grammar point
    for &(g1, g2, note) in CONFUSABLE_GRAMMAR {
        let n1 = normalize_jp(g1);
        let n2 = normalize_jp(g2);
        let target_has_1 = targets.iter().any(|t| *t == n1 || t.ends_with(&n1));
        let target_has_2 = targets.iter().any(|t| *t == n2 || t.ends_with(&n2));
        if (target_has_1 && (norm_in == n2 || norm_in.ends_with(&n2)))
            || (target_has_2 && (norm_in == n1 || norm_in.ends_with(&n1)))
        {
            return GradeOutcome::AlmostRight {
                hint: format!("Almost! {}", note),
                category: "synonym",
            };
        }
    }

    // 5. Minor 1-character kana typo on answers of length >= 3
    if norm_in.chars().count() >= 3
        && targets
            .iter()
            .any(|t| t.chars().count() >= 3 && levenshtein_chars(&norm_in, t) == 1)
    {
        return GradeOutcome::AlmostRight {
            hint: "Almost! Looks like a small typo — double-check your kana and try again."
                .to_string(),
            category: "typo",
        };
    }

    GradeOutcome::Incorrect
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_srs_promotion_and_burn() {
        let now = Utc::now();
        let t1 = compute_srs_transition(0, 0, false, true, now);
        assert_eq!(t1.new_stage, 1);
        assert!(t1.next_review_at.is_some());

        let t_burn = compute_srs_transition(11, 0, false, true, now);
        assert_eq!(t_burn.new_stage, 12);
        assert!(t_burn.next_review_at.is_none());
    }

    #[test]
    fn test_srs_miss_spawns_ghost_by_intensity() {
        let now = Utc::now();
        let min_t =
            compute_srs_transition_with_intensity(6, 0, false, false, GhostIntensity::Minimal, now);
        assert_eq!(min_t.new_stage, 4);
        assert_eq!(min_t.new_ghost_level, 1);

        let std_t = compute_srs_transition(6, 0, false, false, now);
        assert_eq!(std_t.new_ghost_level, 3);

        let heavy_t =
            compute_srs_transition_with_intensity(6, 0, false, false, GhostIntensity::Heavy, now);
        assert_eq!(heavy_t.new_ghost_level, 4);
    }

    #[test]
    fn test_ghost_review_curing() {
        let now = Utc::now();
        let g1 = compute_srs_transition(4, 3, true, true, now);
        assert_eq!(g1.new_stage, 4);
        assert_eq!(g1.new_ghost_level, 2);

        let g2 = compute_srs_transition(4, 1, true, true, now);
        assert_eq!(g2.new_ghost_level, 0);
        assert!(g2.ghost_due_at.is_none());
    }

    #[test]
    fn test_answer_normalization() {
        let alts = vec!["食べている".to_string(), "たべている".to_string()];
        assert!(check_answer(" たべている。", "たべている", &alts).is_some());
        assert!(check_answer("食べている", "たべている", &alts).is_some());
        assert!(check_answer("タベテイル", "たべている", &alts).is_some());
        assert!(check_answer("のんでいる", "たべている", &alts).is_none());
    }

    #[test]
    fn test_contextual_nuance_grading() {
        // Register mismatch: casual typed when polite expected
        match grade_answer("ている", "ています", &[], "polite • ongoing") {
            GradeOutcome::AlmostRight { category, .. } => assert_eq!(category, "register"),
            other => panic!("expected register AlmostRight, got {:?}", other),
        }

        // Tense mismatch: present typed when past expected
        match grade_answer("ています", "ていました", &[], "polite • past continuous") {
            GradeOutcome::AlmostRight { category, .. } => assert_eq!(category, "tense"),
            other => panic!("expected tense AlmostRight, got {:?}", other),
        }

        // Polarity mismatch: affirmative typed when negative expected
        match grade_answer("ことができます", "ことができません", &[], "polite negative") {
            GradeOutcome::AlmostRight { category, .. } => assert_eq!(category, "polarity"),
            other => panic!("expected polarity AlmostRight, got {:?}", other),
        }

        // Confusable grammar synonym: せいで vs おかげで
        match grade_answer("せいで", "おかげで", &[], "positive gratitude") {
            GradeOutcome::AlmostRight { category, .. } => assert_eq!(category, "synonym"),
            other => panic!("expected synonym AlmostRight, got {:?}", other),
        }

        // 1-char kana typo
        match grade_answer("ていむす", "ています", &[], "polite") {
            GradeOutcome::AlmostRight { category, .. } => assert_eq!(category, "typo"),
            other => panic!("expected typo AlmostRight, got {:?}", other),
        }

        // Completely wrong answer -> Incorrect
        assert_eq!(
            grade_answer("まちがい", "ています", &[], "polite"),
            GradeOutcome::Incorrect
        );
    }
}

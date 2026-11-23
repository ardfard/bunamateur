use chrono::{DateTime, Duration, Utc};

/// Bunpro-inspired 12-stage SRS intervals.
/// Stage 0 = New/Unlearned, Stage 12 = Burned (Mastered).
pub fn srs_interval_hours(stage: i32) -> Option<i64> {
    match stage {
        1 => Some(4),         // Apprentice I: 4h
        2 => Some(8),         // Apprentice II: 8h
        3 => Some(24),        // Apprentice III: 1d
        4 => Some(48),        // Apprentice IV: 2d
        5 => Some(96),        // Guru I: 4d
        6 => Some(192),       // Guru II: 8d
        7 => Some(336),       // Master I: 14d
        8 => Some(720),       // Master II: 30d
        9 => Some(1440),      // Enlightened I: 60d
        10 => Some(2880),     // Enlightened II: 120d
        11 => Some(4320),     // Seasoned: 180d
        _ => None,            // Stage 0 (New) or 12+ (Burned)
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

pub struct SrsTransition {
    pub new_stage: i32,
    pub next_review_at: Option<DateTime<Utc>>,
    pub new_ghost_level: i32,
    pub ghost_due_at: Option<DateTime<Utc>>,
}

/// Computes next SRS stage & Ghost Review scheduling.
pub fn compute_srs_transition(
    current_stage: i32,
    current_ghost_level: i32,
    is_ghost_review: bool,
    correct: bool,
    now: DateTime<Utc>,
) -> SrsTransition {
    if is_ghost_review {
        if correct {
            let next_ghost = (current_ghost_level - 1).max(0);
            let ghost_due = match next_ghost {
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
            SrsTransition {
                new_stage: current_stage,
                next_review_at: None,
                new_ghost_level: 3,
                ghost_due_at: Some(now + Duration::hours(4)),
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
        // Spawn a Ghost Review (level 3) due in 4 hours!
        SrsTransition {
            new_stage,
            next_review_at: next_review,
            new_ghost_level: 3,
            ghost_due_at: Some(now + Duration::hours(4)),
        }
    }
}

/// Normalize Japanese answer text for forgiving yet accurate grading.
pub fn normalize_jp(input: &str) -> String {
    input
        .trim()
        .chars()
        .filter(|c| !c.is_whitespace() && !matches!(c, '。' | '、' | '！' | '？' | '.' | ',' | '!' | '?' | '〜' | '～' | '「' | '」'))
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
    fn test_srs_miss_spawns_ghost() {
        let now = Utc::now();
        let t = compute_srs_transition(6, 0, false, false, now);
        assert_eq!(t.new_stage, 4);
        assert_eq!(t.new_ghost_level, 3);
        assert!(t.ghost_due_at.is_some());
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
}

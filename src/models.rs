use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrammarSentence {
    pub id: i64,
    pub grammar_id: i64,
    pub jp_before: String,
    pub jp_after: String,
    pub answer: String,
    pub alt_answers: Vec<String>,
    pub hint: String,
    pub english: String,
    pub nuance_note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrammarPointSummary {
    pub id: i64,
    pub slug: String,
    pub title: String,
    pub reading: String,
    pub meaning: String,
    pub jlpt_level: String,
    pub study_path: String,
    pub structure: String,
    pub srs_stage: i32,
    pub srs_label: String,
    pub next_review_at: Option<String>,
    pub ghost_level: i32,
    pub is_leech: bool,
    pub wk_readiness_pct: u8,
    pub wk_known_kanji: usize,
    pub wk_total_kanji: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrammarPointDetail {
    pub id: i64,
    pub slug: String,
    pub title: String,
    pub reading: String,
    pub meaning: String,
    pub jlpt_level: String,
    pub study_path: String,
    pub structure: String,
    pub nuance: String,
    pub caution: String,
    pub confusable_with: String,
    pub pitch_note: String,
    pub user_notes: String,
    pub srs_stage: i32,
    pub srs_label: String,
    pub correct_count: i32,
    pub incorrect_count: i32,
    pub streak: i32,
    pub ghost_level: i32,
    pub next_review_at: Option<String>,
    pub sentences: Vec<GrammarSentence>,
    pub wk_readiness_pct: u8,
    pub wk_known_kanji: usize,
    pub wk_total_kanji: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewQueueItem {
    pub grammar_id: i64,
    pub sentence_id: i64,
    pub title: String,
    pub reading: String,
    pub meaning: String,
    pub jlpt_level: String,
    pub structure: String,
    pub nuance: String,
    pub caution: String,
    pub confusable_with: String,
    pub jp_before: String,
    pub jp_after: String,
    pub answer: String,
    pub alt_answers: Vec<String>,
    pub hint: String,
    pub english: String,
    pub nuance_note: String,
    pub srs_stage: i32,
    pub srs_label: String,
    pub is_ghost: bool,
    pub ghost_level: i32,
    pub mode: String, // "review" | "lesson" | "cram"
}

#[derive(Debug, Clone, Deserialize)]
pub struct SubmitAnswerRequest {
    pub grammar_id: i64,
    pub sentence_id: i64,
    pub user_input: String,
    pub mode: String, // "review" | "lesson" | "cram"
    pub is_ghost: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SubmitAnswerResponse {
    pub correct: bool,
    pub matched_answer: String,
    pub expected_answer: String,
    pub alt_answers: Vec<String>,
    pub previous_srs_stage: i32,
    pub new_srs_stage: i32,
    pub new_srs_label: String,
    pub next_review_at: Option<String>,
    pub previous_ghost_level: i32,
    pub new_ghost_level: i32,
    pub feedback: String,
    pub review_log_id: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UndoReviewRequest {
    pub grammar_id: i64,
    pub previous_srs_stage: i32,
    pub previous_ghost_level: i32,
    pub review_log_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct JlptProgress {
    pub level: String,
    pub learned: i32,
    pub total: i32,
    pub mastered: i32,
    pub percent: u8,
}

#[derive(Debug, Clone, Serialize)]
pub struct ForecastBucket {
    pub label: String,
    pub count: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct SrsTierCount {
    pub tier: String,
    pub range: String,
    pub count: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct DashboardStats {
    pub reviews_due: i32,
    pub ghost_reviews_due: i32,
    pub lessons_available: i32,
    pub total_learned: i32,
    pub total_grammar: i32,
    pub accuracy_7d: u8,
    pub reviews_7d: i32,
    pub streak_days: i32,
    pub daily_goal: i32,
    pub today_reviews: i32,
    pub leech_count: i32,
    pub jlpt_progress: Vec<JlptProgress>,
    pub forecast_24h: Vec<ForecastBucket>,
    pub srs_breakdown: Vec<SrsTierCount>,
    pub wanikani: WaniKaniStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaniKaniStatus {
    pub connected: bool,
    pub is_demo: bool,
    pub username: Option<String>,
    pub level: i32,
    pub kanji_known: usize,
    pub vocab_known: usize,
    pub wk_lessons_available: i32,
    pub wk_reviews_available: i32,
    pub smart_furigana: bool,
    pub highlight_known_kanji: bool,
    pub last_synced_at: Option<String>,
    pub known_kanji_list: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WaniKaniConnectRequest {
    pub api_token: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WaniKaniSettingsRequest {
    pub smart_furigana: bool,
    pub highlight_known_kanji: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateNotesRequest {
    pub notes: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StartLessonRequest {
    pub grammar_id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadingPassage {
    pub id: i64,
    pub title: String,
    pub jlpt_level: String,
    pub topic: String,
    pub content_ruby: String,
    pub english: String,
    pub wk_readiness_pct: u8,
    pub detected_grammar: Vec<DetectedGrammar>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReadingAnalyzeRequest {
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedGrammar {
    pub grammar_id: i64,
    pub title: String,
    pub meaning: String,
    pub jlpt_level: String,
    pub srs_stage: i32,
    pub srs_label: String,
    pub matched_token: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReadingAnalyzeResponse {
    pub total_kanji: usize,
    pub known_kanji: usize,
    pub unknown_kanji: Vec<String>,
    pub wk_readiness_pct: u8,
    pub detected_grammar: Vec<DetectedGrammar>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConjugationItem {
    pub id: i64,
    pub dictionary: String,
    pub reading: String,
    pub meaning: String,
    pub word_type: String, // "ichidan" | "godan" | "irregular" | "i-adj" | "na-adj"
    pub target_form: String, // "te" | "nai" | "ta" | "masu" | "potential" | "ba" | "passive" | "causative"
    pub target_label: String,
    pub answer_kana: String,
    pub answer_kanji: String,
    pub rule_explanation: String,
    pub example_jp: String,
    pub example_en: String,
}

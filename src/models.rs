use crate::lesson::{GrammarLesson, LessonMistake};
use crate::nlp::AnalyzedToken;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthUserInfo {
    pub id: i64,
    pub email: String,
    pub username: String,
    pub avatar_url: Option<String>,
    #[serde(default)]
    pub email_verified: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthProvidersResponse {
    pub google: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RegisterRequest {
    pub email: String,
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthResponse {
    pub user: AuthUserInfo,
    pub csrf_token: String,
}

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
    /// Curriculum unit id when assigned; `None` for unplaced / sketch points.
    pub unit_id: Option<String>,
    /// Display title of the curriculum unit (JOIN); `None` when unplaced.
    pub unit_title: Option<String>,
    /// Teaching sort order within the curriculum; `None` when unplaced.
    pub sort_order: Option<i32>,
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
    /// Rich lesson when seeded; `None` falls back to short nuance/caution fields.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lesson: Option<GrammarLesson>,
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
    /// Rich lesson for Learn New pre-cloze flow; optional.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lesson: Option<GrammarLesson>,
    /// Convenience copy of lesson mistakes for post-miss feedback (also inside `lesson`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mistakes: Vec<LessonMistake>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SubmitAnswerRequest {
    pub grammar_id: i64,
    pub sentence_id: i64,
    pub user_input: String,
    pub mode: String, // "review" | "lesson" | "cram"
    pub is_ghost: bool,
    /// How many AlmostRight free retries the client already got for this card.
    #[serde(default)]
    pub near_miss_attempts: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct SubmitAnswerResponse {
    pub correct: bool,
    pub almost_right: bool,
    pub nuance_hint: Option<String>,
    pub nuance_category: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnboardingState {
    pub completed: bool,
    pub preferred_study_path: String,
    pub placement_jlpt: String,
    pub daily_goal: i32,
    pub content_version: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SrsSettings {
    pub vacation_mode: bool,
    pub vacation_started_at: Option<String>,
    pub ghost_intensity: String, // "minimal" | "standard" | "heavy"
    pub daily_goal: i32,
    pub preferred_study_path: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateSrsSettingsRequest {
    pub vacation_mode: Option<bool>,
    pub ghost_intensity: Option<String>,
    pub daily_goal: Option<i32>,
    pub preferred_study_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlacementQuestion {
    pub grammar_id: i64,
    pub slug: String,
    pub title: String,
    pub meaning: String,
    pub jlpt_level: String,
    pub study_path: String,
    pub sample_jp: String,
    pub sample_en: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CompleteOnboardingRequest {
    pub study_path: String,
    pub placement_jlpt: String,
    pub daily_goal: Option<i32>,
    #[serde(default)]
    pub known_grammar_ids: Vec<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentVersionInfo {
    pub content_key: String,
    pub version: i32,
    pub checksum: String,
    pub item_count: i32,
    pub sentence_count: i32,
    pub applied_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressExportRow {
    pub slug: String,
    pub title: String,
    pub jlpt_level: String,
    pub srs_stage: i32,
    pub ghost_level: i32,
    pub correct_count: i32,
    pub incorrect_count: i32,
    pub streak: i32,
    pub next_review_at: Option<String>,
    pub ghost_due_at: Option<String>,
    pub user_notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressExportPayload {
    pub exported_at: String,
    pub version: i32,
    pub settings: SrsSettings,
    pub items: Vec<ProgressExportRow>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ImportProgressRequest {
    pub format: String, // "json" | "csv"
    pub data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportProgressResponse {
    pub imported: usize,
    pub skipped: usize,
    pub errors: Vec<String>,
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
    pub onboarding: OnboardingState,
    pub srs_settings: SrsSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaniKaniStatus {
    pub connected: bool,
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
    pub sync_status: String,
    pub sync_progress_pages: i32,
    pub sync_progress_items: i32,
    pub sync_message: String,
    pub sync_error: Option<String>,
    pub auto_sync_enabled: bool,
    pub auto_sync_interval_mins: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaniKaniSyncStatus {
    pub sync_status: String,
    pub sync_progress_pages: i32,
    pub sync_progress_items: i32,
    pub sync_message: String,
    pub sync_error: Option<String>,
    pub last_synced_at: Option<String>,
    pub kanji_known: usize,
    pub vocab_known: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WaniKaniConnectRequest {
    pub api_token: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WaniKaniSettingsRequest {
    pub smart_furigana: bool,
    pub highlight_known_kanji: bool,
    pub auto_sync_enabled: Option<bool>,
    pub auto_sync_interval_mins: Option<i32>,
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
    pub tokens: Vec<AnalyzedToken>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadingAnalyzeResponse {
    pub total_kanji: usize,
    pub known_kanji: usize,
    pub unknown_kanji: Vec<String>,
    pub wk_readiness_pct: u8,
    pub detected_grammar: Vec<DetectedGrammar>,
    pub tokens: Vec<AnalyzedToken>,
    pub annotated_ruby: String,
    pub analyzer_engine: String,
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

// ---------------------------------------------------------------------------
// v0.2 Account Lifecycle, Study Analytics & Vocabulary Deck Models
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct VerifyEmailRequest {
    pub token: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ForgotPasswordRequest {
    pub email: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResetPasswordRequest {
    pub token: String,
    pub new_password: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateProfileRequest {
    pub username: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChangePasswordRequest {
    pub current_password: Option<String>,
    pub new_password: String,
    pub revoke_other_sessions: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountSessionInfo {
    pub id: String,
    pub is_current: bool,
    pub user_agent: Option<String>,
    pub ip_address: Option<String>,
    pub created_at: String,
    pub last_seen_at: String,
    pub expires_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountProfileResponse {
    pub user: AuthUserInfo,
    pub has_password: bool,
    pub google_linked: bool,
    pub active_sessions: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyActivityPoint {
    pub date: String,
    pub total_reviews: i32,
    pub correct_reviews: i32,
    pub accuracy_pct: u8,
    pub intensity: u8, // 0..=4
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JlptAccuracyRow {
    pub jlpt_level: String,
    pub reviews: i32,
    pub correct: i32,
    pub accuracy_pct: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeechGrammarItem {
    pub grammar_id: i64,
    pub slug: String,
    pub title: String,
    pub meaning: String,
    pub jlpt_level: String,
    pub incorrect_count: i32,
    pub correct_count: i32,
    pub accuracy_pct: u8,
    pub ghost_level: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StudyAnalyticsResponse {
    pub days: i32,
    pub total_reviews_all_time: i32,
    pub overall_accuracy_pct: u8,
    pub longest_streak_days: i32,
    pub active_days_in_window: i32,
    pub heatmap: Vec<DailyActivityPoint>,
    pub accuracy_by_jlpt: Vec<JlptAccuracyRow>,
    pub top_leeches: Vec<LeechGrammarItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VocabBookmark {
    pub id: i64,
    pub surface: String,
    pub lemma: String,
    pub reading: String,
    pub pos: String,
    pub glosses: Vec<String>,
    pub jlpt: Option<String>,
    pub wk_level: Option<i32>,
    pub context_sentence: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SaveVocabBookmarkRequest {
    pub surface: String,
    pub lemma: String,
    pub reading: String,
    pub pos: String,
    #[serde(default)]
    pub glosses: Vec<String>,
    pub jlpt: Option<String>,
    pub wk_level: Option<i32>,
    pub context_sentence: Option<String>,
}

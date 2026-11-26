export interface AuthUser {
  id: number
  email: string
  username: string
  avatar_url?: string | null
  created_at: string
}

export interface AuthProvidersResponse {
  google: boolean
}

export interface AuthResponse {
  user: AuthUser
  csrf_token: string
}

export interface GrammarSentence {
  id: number
  grammar_id: number
  jp_before: string
  jp_after: string
  answer: string
  alt_answers: string[]
  hint: string
  english: string
  nuance_note: string
}

export interface GrammarPointSummary {
  id: number
  slug: string
  title: string
  reading: string
  meaning: string
  jlpt_level: string
  study_path: string
  structure: string
  srs_stage: number
  srs_label: string
  next_review_at: string | null
  ghost_level: number
  is_leech: boolean
  wk_readiness_pct: number
  wk_known_kanji: number
  wk_total_kanji: number
}

export interface GrammarPointDetail {
  id: number
  slug: string
  title: string
  reading: string
  meaning: string
  jlpt_level: string
  study_path: string
  structure: string
  nuance: string
  caution: string
  confusable_with: string
  pitch_note: string
  user_notes: string
  srs_stage: number
  srs_label: string
  correct_count: number
  incorrect_count: number
  streak: number
  ghost_level: number
  next_review_at: string | null
  sentences: GrammarSentence[]
  wk_readiness_pct: number
  wk_known_kanji: number
  wk_total_kanji: number
}

export interface ReviewQueueItem {
  grammar_id: number
  sentence_id: number
  title: string
  reading: string
  meaning: string
  jlpt_level: string
  structure: string
  nuance: string
  caution: string
  confusable_with: string
  jp_before: string
  jp_after: string
  answer: string
  alt_answers: string[]
  hint: string
  english: string
  nuance_note: string
  srs_stage: number
  srs_label: string
  is_ghost: boolean
  ghost_level: number
  mode: 'review' | 'lesson' | 'cram'
}

export interface SubmitAnswerResponse {
  correct: boolean
  almost_right: boolean
  nuance_hint: string | null
  nuance_category: string | null
  matched_answer: string
  expected_answer: string
  alt_answers: string[]
  previous_srs_stage: number
  new_srs_stage: number
  new_srs_label: string
  next_review_at: string | null
  previous_ghost_level: number
  new_ghost_level: number
  feedback: string
  review_log_id: number | null
}

export interface JlptProgress {
  level: string
  learned: number
  total: number
  mastered: number
  percent: number
}

export interface ForecastBucket {
  label: string
  count: number
}

export interface SrsTierCount {
  tier: string
  range: string
  count: number
}

export interface WaniKaniStatus {
  connected: boolean
  is_demo: boolean
  username: string | null
  level: number
  kanji_known: number
  vocab_known: number
  wk_lessons_available: number
  wk_reviews_available: number
  smart_furigana: boolean
  highlight_known_kanji: boolean
  last_synced_at: string | null
  known_kanji_list: string[]
  sync_status: 'idle' | 'running' | 'completed' | 'failed' | string
  sync_progress_pages: number
  sync_progress_items: number
  sync_message: string
  sync_error: string | null
  auto_sync_enabled: boolean
  auto_sync_interval_mins: number
}

export interface WaniKaniSyncStatus {
  sync_status: string
  sync_progress_pages: number
  sync_progress_items: number
  sync_message: string
  sync_error: string | null
  last_synced_at: string | null
  kanji_known: number
  vocab_known: number
}

export interface OnboardingState {
  completed: boolean
  preferred_study_path: string
  placement_jlpt: string
  daily_goal: number
  content_version: number
}

export interface SrsSettings {
  vacation_mode: boolean
  vacation_started_at: string | null
  ghost_intensity: 'minimal' | 'standard' | 'heavy' | string
  daily_goal: number
  preferred_study_path: string
}

export interface PlacementQuestion {
  grammar_id: number
  slug: string
  title: string
  meaning: string
  jlpt_level: string
  study_path: string
  sample_jp: string
  sample_en: string
}

export interface ImportProgressResponse {
  imported: number
  skipped: number
  errors: string[]
}

export interface DashboardStats {
  reviews_due: number
  ghost_reviews_due: number
  lessons_available: number
  total_learned: number
  total_grammar: number
  accuracy_7d: number
  reviews_7d: number
  streak_days: number
  daily_goal: number
  today_reviews: number
  leech_count: number
  jlpt_progress: JlptProgress[]
  forecast_24h: ForecastBucket[]
  srs_breakdown: SrsTierCount[]
  wanikani: WaniKaniStatus
  onboarding: OnboardingState
  srs_settings: SrsSettings
}

export interface DetectedGrammar {
  grammar_id: number
  title: string
  meaning: string
  jlpt_level: string
  srs_stage: number
  srs_label: string
  matched_token: string
}

export interface AnalyzedToken {
  surface: string
  lemma: string
  reading: string
  pos: string
  glosses: string[]
  jlpt: string | null
  ruby: string
  wk_level: number | null
  wk_known: boolean
  is_content_word: boolean
}

export interface ReadingPassage {
  id: number
  title: string
  jlpt_level: string
  topic: string
  content_ruby: string
  english: string
  wk_readiness_pct: number
  detected_grammar: DetectedGrammar[]
  tokens: AnalyzedToken[]
}

export interface ReadingAnalyzeResponse {
  total_kanji: number
  known_kanji: number
  unknown_kanji: string[]
  wk_readiness_pct: number
  detected_grammar: DetectedGrammar[]
  tokens: AnalyzedToken[]
  annotated_ruby: string
  analyzer_engine: string
}

export interface ConjugationItem {
  id: number
  dictionary: string
  reading: string
  meaning: string
  word_type: string
  target_form: string
  target_label: string
  answer_kana: string
  answer_kanji: string
  rule_explanation: string
  example_jp: string
  example_en: string
}

export type PageTab = 'dashboard' | 'reviews' | 'grammar' | 'conjugation' | 'reading' | 'wanikani'
export type ReviewMode = 'review' | 'lesson' | 'cram' | 'ghost'
export type FuriganaMode = 'smart' | 'all' | 'none'

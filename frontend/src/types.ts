export interface AuthUser {
  id: number
  email: string
  username: string
  created_at: string
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

export interface ReadingPassage {
  id: number
  title: string
  jlpt_level: string
  topic: string
  content_ruby: string
  english: string
  wk_readiness_pct: number
  detected_grammar: DetectedGrammar[]
}

export interface ReadingAnalyzeResponse {
  total_kanji: number
  known_kanji: number
  unknown_kanji: string[]
  wk_readiness_pct: number
  detected_grammar: DetectedGrammar[]
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

import type {
  ConjugationItem,
  DashboardStats,
  GrammarPointDetail,
  GrammarPointSummary,
  ReadingAnalyzeResponse,
  ReadingPassage,
  ReviewMode,
  ReviewQueueItem,
  SubmitAnswerResponse,
  WaniKaniStatus,
} from './types'

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(path, {
    headers: { 'Content-Type': 'application/json', ...(init?.headers || {}) },
    ...init,
  })
  const data = await res.json().catch(() => ({}))
  if (!res.ok) {
    throw new Error(data.error || `Request failed (${res.status})`)
  }
  return data as T
}

export const api = {
  getDashboard: () => request<DashboardStats>('/api/dashboard'),

  getReviews: (mode: ReviewMode, jlpt = 'ALL', limit = 20) =>
    request<ReviewQueueItem[]>(
      `/api/reviews?mode=${encodeURIComponent(mode)}&jlpt=${encodeURIComponent(jlpt)}&limit=${limit}`
    ),

  submitReview: (payload: {
    grammar_id: number
    sentence_id: number
    user_input: string
    mode: string
    is_ghost: boolean
  }) =>
    request<SubmitAnswerResponse>('/api/reviews/submit', {
      method: 'POST',
      body: JSON.stringify(payload),
    }),

  undoReview: (payload: {
    grammar_id: number
    previous_srs_stage: number
    previous_ghost_level: number
    review_log_id: number | null
  }) =>
    request<{ ok: boolean }>('/api/reviews/undo', {
      method: 'POST',
      body: JSON.stringify(payload),
    }),

  startLesson: (grammar_id: number) =>
    request<{ ok: boolean }>('/api/lessons/start', {
      method: 'POST',
      body: JSON.stringify({ grammar_id }),
    }),

  getGrammarList: (params?: { jlpt?: string; q?: string; status?: string }) => {
    const qs = new URLSearchParams()
    if (params?.jlpt) qs.set('jlpt', params.jlpt)
    if (params?.q) qs.set('q', params.q)
    if (params?.status) qs.set('status', params.status)
    return request<GrammarPointSummary[]>(`/api/grammar?${qs.toString()}`)
  },

  getGrammarDetail: (id: number) => request<GrammarPointDetail>(`/api/grammar/${id}`),

  saveGrammarNotes: (id: number, notes: string) =>
    request<{ ok: boolean }>(`/api/grammar/${id}/notes`, {
      method: 'PUT',
      body: JSON.stringify({ notes }),
    }),

  resetGrammarProgress: (id: number) =>
    request<{ ok: boolean }>(`/api/grammar/${id}/reset`, {
      method: 'POST',
    }),

  getReadingPassages: () => request<ReadingPassage[]>('/api/reading/passages'),

  analyzeReading: (text: string) =>
    request<ReadingAnalyzeResponse>('/api/reading/analyze', {
      method: 'POST',
      body: JSON.stringify({ text }),
    }),

  getConjugations: (form = 'all') =>
    request<ConjugationItem[]>(`/api/conjugation?form=${encodeURIComponent(form)}`),

  getWaniKaniStatus: () => request<WaniKaniStatus>('/api/wanikani/status'),

  connectWaniKani: (api_token: string) =>
    request<WaniKaniStatus>('/api/wanikani/connect', {
      method: 'POST',
      body: JSON.stringify({ api_token }),
    }),

  syncWaniKani: () =>
    request<WaniKaniStatus>('/api/wanikani/sync', { method: 'POST' }),

  demoWaniKani: () =>
    request<WaniKaniStatus>('/api/wanikani/demo', { method: 'POST' }),

  disconnectWaniKani: () =>
    request<WaniKaniStatus>('/api/wanikani/disconnect', { method: 'POST' }),

  updateWaniKaniSettings: (smart_furigana: boolean, highlight_known_kanji: boolean) =>
    request<WaniKaniStatus>('/api/wanikani/settings', {
      method: 'PUT',
      body: JSON.stringify({ smart_furigana, highlight_known_kanji }),
    }),
}

import React, { useEffect, useState } from 'react'
import { BookOpen, RotateCcw, Save, Volume2, X, AlertTriangle, GitCompare } from 'lucide-react'
import { api } from '../api'
import type { FuriganaMode, GrammarPointDetail } from '../types'
import { RubyText } from '../utils/furigana'
import { speakJapanese } from '../utils/speech'

interface GrammarDetailModalProps {
  grammarId: number | null
  onClose: () => void
  onChanged: () => void
  furiganaMode: FuriganaMode
  knownKanji: Set<string>
  highlightKnown: boolean
}

export const GrammarDetailModal: React.FC<GrammarDetailModalProps> = ({
  grammarId,
  onClose,
  onChanged,
  furiganaMode,
  knownKanji,
  highlightKnown,
}) => {
  const [detail, setDetail] = useState<GrammarPointDetail | null>(null)
  const [notes, setNotes] = useState('')
  const [saving, setSaving] = useState(false)
  const [savedToast, setSavedToast] = useState(false)
  const [loading, setLoading] = useState(false)

  useEffect(() => {
    if (!grammarId) return
    setLoading(true)
    api
      .getGrammarDetail(grammarId)
      .then((d) => {
        setDetail(d)
        setNotes(d.user_notes || '')
      })
      .finally(() => setLoading(false))
  }, [grammarId])

  if (!grammarId) return null

  const handleSaveNotes = async () => {
    if (!detail) return
    setSaving(true)
    await api.saveGrammarNotes(detail.id, notes)
    setSaving(false)
    setSavedToast(true)
    setTimeout(() => setSavedToast(false), 1800)
  }

  const handleEnroll = async () => {
    if (!detail) return
    await api.startLesson(detail.id)
    const updated = await api.getGrammarDetail(detail.id)
    setDetail(updated)
    onChanged()
  }

  const handleReset = async () => {
    if (!detail) return
    await api.resetGrammarProgress(detail.id)
    const updated = await api.getGrammarDetail(detail.id)
    setDetail(updated)
    onChanged()
  }

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4 backdrop-blur-xs"
      onClick={onClose}
    >
      <div
        className="max-h-[90vh] w-full max-w-3xl overflow-y-auto rounded-2xl border p-6 sm:p-8 shadow-2xl"
        style={{
          backgroundColor: 'var(--bg-surface)',
          borderColor: 'var(--border-subtle)',
        }}
        onClick={(e) => e.stopPropagation()}
      >
        {loading || !detail ? (
          <div className="py-16 text-center text-sm" style={{ color: 'var(--text-muted)' }}>
            Loading grammar reference...
          </div>
        ) : (
          <div className="flex flex-col gap-6">
            {/* Header */}
            <div className="flex items-start justify-between gap-4">
              <div className="flex flex-col gap-1.5">
                <div className="flex flex-wrap items-center gap-2">
                  <span
                    className="font-mono-code rounded-md px-2 py-0.5 text-xs font-bold"
                    style={{
                      backgroundColor: 'var(--accent-indigo-soft)',
                      color: 'var(--accent-indigo)',
                    }}
                  >
                    {detail.jlpt_level}
                  </span>
                  <span
                    className="font-mono-code rounded-md px-2 py-0.5 text-xs font-medium"
                    style={{
                      backgroundColor: 'var(--bg-subtle)',
                      color: 'var(--text-secondary)',
                    }}
                  >
                    {detail.study_path}
                  </span>
                  <span
                    className="font-mono-code rounded-md px-2 py-0.5 text-xs font-semibold"
                    style={{
                      backgroundColor:
                        detail.srs_stage > 0 ? 'var(--accent-matcha-soft)' : 'var(--bg-subtle)',
                      color:
                        detail.srs_stage > 0 ? 'var(--accent-matcha)' : 'var(--text-muted)',
                    }}
                  >
                    {detail.srs_label}
                  </span>
                  {detail.wk_total_kanji > 0 && (
                    <span
                      className="font-mono-code rounded-md px-2 py-0.5 text-xs font-semibold"
                      style={{
                        backgroundColor: 'var(--wk-pink-soft)',
                        color: 'var(--wk-pink)',
                      }}
                    >
                      {detail.wk_readiness_pct}% WK Kanji ({detail.wk_known_kanji}/
                      {detail.wk_total_kanji})
                    </span>
                  )}
                </div>
                <h2 className="font-jp text-3xl font-bold">{detail.title}</h2>
                <p className="text-base" style={{ color: 'var(--text-secondary)' }}>
                  {detail.meaning}
                </p>
              </div>
              <button
                onClick={onClose}
                className="rounded-lg p-2 transition-colors hover:opacity-75 cursor-pointer"
                style={{ backgroundColor: 'var(--bg-subtle)', color: 'var(--text-secondary)' }}
              >
                <X className="h-5 w-5" />
              </button>
            </div>

            {/* Structure Formula */}
            <div
              className="rounded-xl border p-4"
              style={{
                backgroundColor: 'var(--bg-canvas)',
                borderColor: 'var(--border-subtle)',
              }}
            >
              <div
                className="font-mono-code mb-1 text-[11px] font-bold uppercase tracking-wider"
                style={{ color: 'var(--text-muted)' }}
              >
                Structure / Formation
              </div>
              <div className="font-jp text-base font-semibold" style={{ color: 'var(--accent-indigo)' }}>
                {detail.structure}
              </div>
              {detail.pitch_note && (
                <div className="mt-1.5 text-xs" style={{ color: 'var(--text-muted)' }}>
                  Pitch accent: <span className="font-jp font-medium">{detail.pitch_note}</span>
                </div>
              )}
            </div>

            {/* Nuance, Caution & Confusable Contrast */}
            <div className="grid gap-4 sm:grid-cols-2">
              <div
                className="rounded-xl border p-4"
                style={{ borderColor: 'var(--border-subtle)' }}
              >
                <div className="mb-1.5 text-xs font-bold uppercase tracking-wider" style={{ color: 'var(--text-muted)' }}>
                  Explanation & Nuance
                </div>
                <p className="text-sm leading-relaxed" style={{ color: 'var(--text-secondary)' }}>
                  {detail.nuance}
                </p>
              </div>
              <div className="flex flex-col gap-3">
                <div
                  className="rounded-xl border p-3.5"
                  style={{
                    backgroundColor: 'var(--accent-amber-soft)',
                    borderColor: 'var(--border-subtle)',
                  }}
                >
                  <div
                    className="mb-1 flex items-center gap-1.5 text-xs font-bold"
                    style={{ color: 'var(--accent-amber)' }}
                  >
                    <AlertTriangle className="h-3.5 w-3.5" />
                    Caution
                  </div>
                  <p className="text-xs leading-relaxed" style={{ color: 'var(--text-primary)' }}>
                    {detail.caution}
                  </p>
                </div>
                <div
                  className="rounded-xl border p-3.5"
                  style={{
                    backgroundColor: 'var(--accent-indigo-soft)',
                    borderColor: 'var(--border-subtle)',
                  }}
                >
                  <div
                    className="mb-1 flex items-center gap-1.5 text-xs font-bold"
                    style={{ color: 'var(--accent-indigo)' }}
                  >
                    <GitCompare className="h-3.5 w-3.5" />
                    Confusable Contrast
                  </div>
                  <p className="text-xs leading-relaxed" style={{ color: 'var(--text-primary)' }}>
                    {detail.confusable_with}
                  </p>
                </div>
              </div>
            </div>

            {/* Example Sentences */}
            <div className="flex flex-col gap-3">
              <div className="text-xs font-bold uppercase tracking-wider" style={{ color: 'var(--text-muted)' }}>
                Example Sentences ({detail.sentences.length})
              </div>
              {detail.sentences.map((s) => {
                const fullMarkup = `${s.jp_before}${s.answer}${s.jp_after}`
                return (
                  <div
                    key={s.id}
                    className="flex flex-col gap-1.5 rounded-xl border p-4"
                    style={{
                      backgroundColor: 'var(--bg-canvas)',
                      borderColor: 'var(--border-subtle)',
                    }}
                  >
                    <div className="flex items-start justify-between gap-3">
                      <div className="font-jp text-lg leading-loose">
                        <RubyText
                          text={s.jp_before}
                          furiganaMode={furiganaMode}
                          knownKanji={knownKanji}
                          highlightKnown={highlightKnown}
                        />
                        <span className="grammar-highlight mx-0.5">{s.answer}</span>
                        <RubyText
                          text={s.jp_after}
                          furiganaMode={furiganaMode}
                          knownKanji={knownKanji}
                          highlightKnown={highlightKnown}
                        />
                      </div>
                      <button
                        onClick={() => speakJapanese(fullMarkup)}
                        className="shrink-0 rounded-lg p-2 transition-colors hover:opacity-80 cursor-pointer"
                        style={{
                          backgroundColor: 'var(--bg-surface)',
                          color: 'var(--accent-indigo)',
                        }}
                        title="Listen (Japanese TTS)"
                      >
                        <Volume2 className="h-4 w-4" />
                      </button>
                    </div>
                    <p className="text-sm" style={{ color: 'var(--text-secondary)' }}>
                      {s.english}
                    </p>
                    <p className="text-xs" style={{ color: 'var(--text-muted)' }}>
                      {s.nuance_note}
                    </p>
                  </div>
                )
              })}
            </div>

            {/* Personal Mnemonic Notes */}
            <div className="flex flex-col gap-2">
              <div className="flex items-center justify-between">
                <label className="text-xs font-bold uppercase tracking-wider" style={{ color: 'var(--text-muted)' }}>
                  Personal Study Notes / Mnemonics
                </label>
                {savedToast && (
                  <span className="text-xs font-semibold" style={{ color: 'var(--accent-matcha)' }}>
                    Saved!
                  </span>
                )}
              </div>
              <textarea
                rows={2}
                value={notes}
                onChange={(e) => setNotes(e.target.value)}
                placeholder="Write your own mnemonic, example from anime/podcasts, or nuance reminder..."
                className="w-full rounded-xl border p-3 text-sm outline-none"
                style={{
                  backgroundColor: 'var(--bg-canvas)',
                  borderColor: 'var(--border-subtle)',
                  color: 'var(--text-primary)',
                }}
              />
              <div className="flex flex-wrap items-center justify-between gap-3 pt-1">
                <div className="flex items-center gap-2">
                  {detail.srs_stage === 0 ? (
                    <button
                      onClick={handleEnroll}
                      className="flex items-center gap-1.5 rounded-lg px-4 py-2 text-xs font-semibold text-white cursor-pointer"
                      style={{ backgroundColor: 'var(--accent-indigo)' }}
                    >
                      <BookOpen className="h-3.5 w-3.5" />
                      Add to Review Queue
                    </button>
                  ) : (
                    <button
                      onClick={handleReset}
                      className="flex items-center gap-1.5 rounded-lg border px-3 py-2 text-xs font-medium cursor-pointer"
                      style={{
                        borderColor: 'var(--border-subtle)',
                        color: 'var(--text-secondary)',
                      }}
                    >
                      <RotateCcw className="h-3.5 w-3.5" />
                      Reset SRS Progress
                    </button>
                  )}
                </div>
                <button
                  onClick={handleSaveNotes}
                  disabled={saving}
                  className="flex items-center gap-1.5 rounded-lg px-4 py-2 text-xs font-semibold text-white cursor-pointer"
                  style={{ backgroundColor: 'var(--accent-vermilion)' }}
                >
                  <Save className="h-3.5 w-3.5" />
                  {saving ? 'Saving...' : 'Save Notes'}
                </button>
              </div>
            </div>
          </div>
        )}
      </div>
    </div>
  )
}

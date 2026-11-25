import React, { useEffect, useRef, useState } from 'react'
import {
  AlertTriangle,
  ArrowRight,
  CheckCircle2,
  Eye,
  Flag,
  Ghost,
  Keyboard,
  RotateCcw,
  Volume2,
  XCircle,
  Sparkles,
} from 'lucide-react'
import { api } from '../api'
import type {
  FuriganaMode,
  ReviewMode,
  ReviewQueueItem,
  SubmitAnswerResponse,
} from '../types'
import { RubyText } from '../utils/furigana'
import { toHiraganaLive } from '../utils/romaji'
import { speakJapanese } from '../utils/speech'

interface ReviewPageProps {
  initialMode: ReviewMode
  furiganaMode: FuriganaMode
  onChangeFuriganaMode: (m: FuriganaMode) => void
  knownKanji: Set<string>
  highlightKnown: boolean
  onSessionComplete: () => void
}

export const ReviewPage: React.FC<ReviewPageProps> = ({
  initialMode,
  furiganaMode,
  onChangeFuriganaMode,
  knownKanji,
  highlightKnown,
  onSessionComplete,
}) => {
  const [mode, setMode] = useState<ReviewMode>(initialMode)
  const [jlptFilter, setJlptFilter] = useState('ALL')
  const [queue, setQueue] = useState<ReviewQueueItem[]>([])
  const [index, setIndex] = useState(0)
  const [loading, setLoading] = useState(true)

  const [inputValue, setInputValue] = useState('')
  const [imeEnabled, setImeEnabled] = useState(true)
  const [showHint, setShowHint] = useState(false)
  const [result, setResult] = useState<SubmitAnswerResponse | null>(null)
  const [nuanceShakeHint, setNuanceShakeHint] = useState<string | null>(null)
  const [shaking, setShaking] = useState(false)
  const [wrappingUp, setWrappingUp] = useState(false)
  const [submitting, setSubmitting] = useState(false)
  const [sessionStats, setSessionStats] = useState({ correct: 0, total: 0 })

  const inputRef = useRef<HTMLInputElement>(null)

  const loadQueue = async (m: ReviewMode, jlpt: string) => {
    setLoading(true)
    setIndex(0)
    setResult(null)
    setNuanceShakeHint(null)
    setWrappingUp(false)
    setInputValue('')
    setShowHint(false)
    setSessionStats({ correct: 0, total: 0 })
    try {
      const items = await api.getReviews(m, jlpt, 20)
      setQueue(items)
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    setMode(initialMode)
  }, [initialMode])

  useEffect(() => {
    loadQueue(mode, jlptFilter)
  }, [mode, jlptFilter])

  useEffect(() => {
    if (!result && !loading) {
      inputRef.current?.focus()
    }
  }, [index, result, loading])

  const current = queue[index]

  const handleInputChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const raw = e.target.value
    setInputValue(imeEnabled ? toHiraganaLive(raw, false) : raw)
  }

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    if (!current || submitting) return

    if (result) {
      goNext()
      return
    }

    const finalInput = imeEnabled ? toHiraganaLive(inputValue, true) : inputValue
    if (!finalInput.trim()) return

    setInputValue(finalInput)
    setSubmitting(true)
    try {
      const res = await api.submitReview({
        grammar_id: current.grammar_id,
        sentence_id: current.sentence_id,
        user_input: finalInput,
        mode: current.mode,
        is_ghost: current.is_ghost,
      })

      // Non-penalizing "almost right" nuance hint: shake input and let user retry immediately
      if (res.almost_right) {
        setNuanceShakeHint(res.nuance_hint || res.feedback)
        setShaking(false)
        requestAnimationFrame(() => setShaking(true))
        window.setTimeout(() => setShaking(false), 400)
        inputRef.current?.focus()
        return
      }

      setNuanceShakeHint(null)
      setResult(res)
      setSessionStats((prev) => ({
        correct: prev.correct + (res.correct ? 1 : 0),
        total: prev.total + 1,
      }))
      onSessionComplete()
    } finally {
      setSubmitting(false)
    }
  }

  const goNext = () => {
    setResult(null)
    setNuanceShakeHint(null)
    setInputValue('')
    setShowHint(false)
    setIndex((i) => i + 1)
  }

  const handleWrapUp = () => {
    if (wrappingUp) return
    const cap = Math.min(queue.length, index + 5)
    setQueue((prev) => prev.slice(0, cap))
    setWrappingUp(true)
  }

  const handleUndo = async () => {
    if (!current || !result) return
    await api.undoReview({
      grammar_id: current.grammar_id,
      previous_srs_stage: result.previous_srs_stage,
      previous_ghost_level: result.previous_ghost_level,
      review_log_id: result.review_log_id,
    })
    setSessionStats((prev) => ({
      correct: Math.max(0, prev.correct - (result.correct ? 1 : 0)),
      total: Math.max(0, prev.total - 1),
    }))
    setResult(null)
    setInputValue('')
    onSessionComplete()
    setTimeout(() => inputRef.current?.focus(), 30)
  }

  const progressPct = queue.length > 0 ? Math.min(100, Math.round((index / queue.length) * 100)) : 0
  const sessionAcc =
    sessionStats.total > 0 ? Math.round((sessionStats.correct / sessionStats.total) * 100) : 100

  return (
    <div className="flex flex-col">
      {/* Top Progress Line */}
      <div className="h-1.5 w-full" style={{ backgroundColor: 'var(--bg-subtle)' }}>
        <div
          className="h-full transition-all duration-300"
          style={{ width: `${progressPct}%`, backgroundColor: 'var(--accent-vermilion)' }}
        />
      </div>

      <div className="mx-auto flex w-full max-w-[980px] flex-col gap-6 px-4 py-8 sm:px-6">
        {/* Mode Switcher & Controls Bar */}
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div className="flex flex-wrap items-center gap-1.5">
            {(
              [
                ['review', 'Due Reviews'],
                ['ghost', 'Ghost Reviews'],
                ['lesson', 'Learn New'],
                ['cram', 'Cram Mode'],
              ] as [ReviewMode, string][]
            ).map(([m, label]) => (
              <button
                key={m}
                onClick={() => setMode(m)}
                className="rounded-full border px-3.5 py-1.5 text-xs font-semibold transition-colors cursor-pointer"
                style={{
                  backgroundColor: mode === m ? 'var(--accent-vermilion)' : 'var(--bg-surface)',
                  borderColor: mode === m ? 'var(--accent-vermilion)' : 'var(--border-subtle)',
                  color: mode === m ? '#ffffff' : 'var(--text-secondary)',
                }}
              >
                {label}
              </button>
            ))}

            {(mode === 'cram' || mode === 'lesson') && (
              <select
                value={jlptFilter}
                onChange={(e) => setJlptFilter(e.target.value)}
                className="font-mono-code rounded-full border px-3 py-1.5 text-xs font-semibold outline-none"
                style={{
                  backgroundColor: 'var(--bg-surface)',
                  borderColor: 'var(--border-subtle)',
                  color: 'var(--text-primary)',
                }}
              >
                {['ALL', 'N5', 'N4', 'N3', 'N2', 'N1'].map((lvl) => (
                  <option key={lvl} value={lvl}>
                    JLPT: {lvl}
                  </option>
                ))}
              </select>
            )}
          </div>

          <div className="flex flex-wrap items-center gap-2">
            {/* Smart Furigana Switcher */}
            <div
              className="flex items-center rounded-full border p-0.5 text-[11px] font-semibold"
              style={{
                backgroundColor: 'var(--bg-surface)',
                borderColor: 'var(--border-subtle)',
              }}
            >
              {(
                [
                  ['smart', 'WK Smart'],
                  ['all', 'All Furigana'],
                  ['none', 'No Furigana'],
                ] as [FuriganaMode, string][]
              ).map(([fm, lbl]) => (
                <button
                  key={fm}
                  onClick={() => onChangeFuriganaMode(fm)}
                  className="rounded-full px-2.5 py-1 transition-colors cursor-pointer"
                  style={{
                    backgroundColor:
                      furiganaMode === fm
                        ? fm === 'smart'
                          ? 'var(--wk-pink-soft)'
                          : 'var(--bg-subtle)'
                        : 'transparent',
                    color:
                      furiganaMode === fm
                        ? fm === 'smart'
                          ? 'var(--wk-pink)'
                          : 'var(--text-primary)'
                        : 'var(--text-muted)',
                  }}
                >
                  {lbl}
                </button>
              ))}
            </div>

            {queue.length > 0 && current && (
              <button
                type="button"
                onClick={handleWrapUp}
                disabled={wrappingUp || queue.length - index <= 1}
                className="flex items-center gap-1 rounded-full border px-3 py-1 text-[11px] font-semibold transition-colors cursor-pointer disabled:opacity-60"
                style={{
                  backgroundColor: wrappingUp ? 'var(--accent-amber-soft)' : 'var(--bg-surface)',
                  borderColor: wrappingUp ? 'var(--accent-amber)' : 'var(--border-subtle)',
                  color: wrappingUp ? 'var(--accent-amber)' : 'var(--text-secondary)',
                }}
                title="Cap remaining session to at most 5 items so you can wrap up cleanly"
              >
                <Flag className="h-3 w-3" />
                {wrappingUp ? `Wrapping up (${Math.max(0, queue.length - index)} left)` : 'Wrap Up'}
              </button>
            )}

            {queue.length > 0 && (
              <span className="font-mono-code text-xs" style={{ color: 'var(--text-secondary)' }}>
                {Math.min(index + 1, queue.length)} / {queue.length} • {sessionAcc}%
              </span>
            )}
          </div>
        </div>

        {/* Main Review Stage */}
        {loading ? (
          <div
            className="rounded-2xl border py-20 text-center text-sm"
            style={{
              backgroundColor: 'var(--bg-surface)',
              borderColor: 'var(--border-subtle)',
              color: 'var(--text-muted)',
            }}
          >
            Preparing your Japanese grammar session...
          </div>
        ) : !current ? (
          <div
            className="flex flex-col items-center gap-4 rounded-2xl border px-6 py-16 text-center"
            style={{
              backgroundColor: 'var(--bg-surface)',
              borderColor: 'var(--border-subtle)',
            }}
          >
            <Sparkles className="h-10 w-10" style={{ color: 'var(--accent-matcha)' }} />
            <h2 className="font-jp text-2xl font-bold">
              お疲れ様でした！ Session Complete
            </h2>
            <p className="max-w-md text-sm" style={{ color: 'var(--text-secondary)' }}>
              {sessionStats.total > 0
                ? `You completed ${sessionStats.total} items with ${sessionAcc}% accuracy.`
                : `No items currently queued in ${mode} mode.`}
            </p>
            <div className="flex flex-wrap justify-center gap-3 pt-2">
              <button
                onClick={() => setMode('lesson')}
                className="rounded-xl px-5 py-2.5 text-sm font-semibold text-white cursor-pointer"
                style={{ backgroundColor: 'var(--accent-indigo)' }}
              >
                Learn New Grammar
              </button>
              <button
                onClick={() => setMode('cram')}
                className="rounded-xl border px-5 py-2.5 text-sm font-semibold cursor-pointer"
                style={{
                  backgroundColor: 'var(--bg-subtle)',
                  borderColor: 'var(--border-subtle)',
                }}
              >
                Practice in Cram Mode
              </button>
            </div>
          </div>
        ) : (
          <>
            {/* Session Badges */}
            <div className="flex flex-wrap items-center justify-between gap-2">
              <div className="flex flex-wrap items-center gap-2">
                {current.is_ghost && (
                  <span
                    className="flex items-center gap-1.5 rounded-full px-3 py-1 text-xs font-semibold"
                    style={{
                      backgroundColor: 'var(--accent-vermilion-soft)',
                      color: 'var(--accent-vermilion)',
                    }}
                  >
                    <Ghost className="h-3.5 w-3.5" />
                    Ghost Review • Level {current.ghost_level}
                  </span>
                )}
                {current.mode === 'lesson' && (
                  <span
                    className="rounded-full px-3 py-1 text-xs font-semibold"
                    style={{
                      backgroundColor: 'var(--accent-indigo-soft)',
                      color: 'var(--accent-indigo)',
                    }}
                  >
                    New Lesson — Study Structure Below, Then Type!
                  </span>
                )}
                <span
                  className="font-mono-code rounded-full px-3 py-1 text-xs font-semibold"
                  style={{
                    backgroundColor: 'var(--bg-subtle)',
                    color: 'var(--text-secondary)',
                  }}
                >
                  {current.jlpt_level} • {current.srs_label}
                </span>
              </div>

              <div className="flex items-center gap-2">
                <button
                  type="button"
                  onClick={() =>
                    speakJapanese(`${current.jp_before}${current.answer}${current.jp_after}`, 0.95)
                  }
                  className="flex items-center gap-1.5 rounded-lg border px-3 py-1.5 text-xs font-medium cursor-pointer"
                  style={{
                    backgroundColor: 'var(--bg-surface)',
                    borderColor: 'var(--border-subtle)',
                    color: 'var(--text-secondary)',
                  }}
                  title="Play Japanese Audio"
                >
                  <Volume2 className="h-3.5 w-3.5" />
                  Listen
                </button>
                <button
                  type="button"
                  onClick={() =>
                    speakJapanese(`${current.jp_before}${current.answer}${current.jp_after}`, 0.72)
                  }
                  className="font-mono-code rounded-lg border px-2.5 py-1.5 text-xs font-medium cursor-pointer"
                  style={{
                    backgroundColor: 'var(--bg-surface)',
                    borderColor: 'var(--border-subtle)',
                    color: 'var(--text-muted)',
                  }}
                  title="Slow Pronunciation (0.72x)"
                >
                  0.7x
                </button>
              </div>
            </div>

            {/* Cloze Question Card */}
            <div
              className="flex flex-col items-center gap-6 rounded-2xl border px-6 py-10 text-center sm:px-12"
              style={{
                backgroundColor: 'var(--bg-surface)',
                borderColor: 'var(--border-subtle)',
              }}
            >
              {/* Nuance / Tense Hint */}
              <div
                className="font-mono-code rounded-full px-3.5 py-1 text-xs font-semibold"
                style={{
                  backgroundColor: 'var(--accent-indigo-soft)',
                  color: 'var(--accent-indigo)',
                }}
              >
                {current.hint} {showHint ? ` • 「${current.title}」` : ''}
              </div>

              {/* Japanese Cloze Sentence */}
              <div className="font-jp text-2xl leading-[2.3] font-bold sm:text-3xl">
                <RubyText
                  text={current.jp_before}
                  furiganaMode={furiganaMode}
                  knownKanji={knownKanji}
                  highlightKnown={highlightKnown}
                />
                <span
                  className="mx-1.5 inline-block min-w-[96px] rounded-xl border-2 px-4 py-0.5 align-baseline transition-colors"
                  style={{
                    borderColor: result
                      ? result.correct
                        ? 'var(--accent-matcha)'
                        : 'var(--accent-vermilion)'
                      : 'var(--accent-vermilion)',
                    backgroundColor: result
                      ? result.correct
                        ? 'var(--accent-matcha-soft)'
                        : 'var(--accent-vermilion-soft)'
                      : 'var(--accent-vermilion-soft)',
                    color: result
                      ? result.correct
                        ? 'var(--accent-matcha)'
                        : 'var(--accent-vermilion)'
                      : 'var(--accent-vermilion)',
                  }}
                >
                  {result ? result.expected_answer : inputValue || '＿＿＿＿'}
                </span>
                <RubyText
                  text={current.jp_after}
                  furiganaMode={furiganaMode}
                  knownKanji={knownKanji}
                  highlightKnown={highlightKnown}
                />
              </div>

              {/* English Prompt */}
              <p className="max-w-2xl text-base sm:text-lg" style={{ color: 'var(--text-secondary)' }}>
                {current.english}
              </p>

              {/* Kana IME Input Form */}
              <form onSubmit={handleSubmit} className="w-full max-w-xl">
                <div
                  className={`flex items-center gap-2 rounded-xl border-2 px-4 py-2.5 transition-colors ${shaking ? 'animate-shake' : ''}`}
                  style={{
                    backgroundColor: 'var(--bg-canvas)',
                    borderColor: result
                      ? result.correct
                        ? 'var(--accent-matcha)'
                        : 'var(--accent-vermilion)'
                      : nuanceShakeHint
                        ? 'var(--accent-amber)'
                        : 'var(--border-subtle)',
                  }}
                >
                  <input
                    ref={inputRef}
                    type="text"
                    value={inputValue}
                    onChange={handleInputChange}
                    disabled={Boolean(result)}
                    placeholder={
                      imeEnabled
                        ? 'Type romaji (e.g. noni -> のに) or kana...'
                        : 'Type Japanese directly...'
                    }
                    className="font-jp w-full bg-transparent text-xl font-semibold outline-none"
                    style={{ color: 'var(--text-primary)' }}
                    autoComplete="off"
                    spellCheck={false}
                  />

                  <button
                    type="button"
                    onClick={() => setImeEnabled((v) => !v)}
                    className="font-mono-code shrink-0 flex items-center gap-1 rounded-lg px-2.5 py-1.5 text-[11px] font-semibold cursor-pointer"
                    style={{
                      backgroundColor: imeEnabled ? 'var(--accent-matcha-soft)' : 'var(--bg-subtle)',
                      color: imeEnabled ? 'var(--accent-matcha)' : 'var(--text-muted)',
                    }}
                    title="Toggle Built-in Romaji -> Hiragana IME"
                  >
                    <Keyboard className="h-3.5 w-3.5" />
                    {imeEnabled ? 'あ IME' : 'ABC'}
                  </button>

                  <button
                    type="submit"
                    disabled={submitting}
                    className="shrink-0 flex items-center gap-1.5 rounded-lg px-4 py-2 text-xs font-semibold text-white cursor-pointer"
                    style={{
                      backgroundColor: result
                        ? result.correct
                          ? 'var(--accent-matcha)'
                          : 'var(--accent-vermilion)'
                        : 'var(--accent-vermilion)',
                    }}
                  >
                    {result ? (
                      <>
                        Next <ArrowRight className="h-3.5 w-3.5" />
                      </>
                    ) : (
                      'Check'
                    )}
                  </button>
                </div>

                {/* Action Strip under Input */}
                <div className="mt-3 flex flex-wrap items-center justify-between gap-2 text-xs">
                  <div className="flex items-center gap-3">
                    {!result && (
                      <button
                        type="button"
                        onClick={() => setShowHint((h) => !h)}
                        className="flex items-center gap-1 font-medium hover:underline cursor-pointer"
                        style={{ color: 'var(--text-secondary)' }}
                      >
                        <Eye className="h-3.5 w-3.5" />
                        {showHint ? 'Hide Grammar Hint' : 'Show Grammar Hint'}
                      </button>
                    )}
                    {result && !result.correct && current.mode !== 'cram' && (
                      <button
                        type="button"
                        onClick={handleUndo}
                        className="flex items-center gap-1 font-semibold hover:underline cursor-pointer"
                        style={{ color: 'var(--accent-amber)' }}
                      >
                        <RotateCcw className="h-3.5 w-3.5" />
                        Oops / Typo — Undo & Retry
                      </button>
                    )}
                  </div>
                  <span className="font-mono-code text-[11px]" style={{ color: 'var(--text-muted)' }}>
                    Press Enter to {result ? 'continue' : 'submit'}
                  </span>
                </div>
              </form>

              {/* Non-penalizing Almost-Right Nuance Hint Banner */}
              {nuanceShakeHint && !result && (
                <div
                  className="flex w-full max-w-xl items-center gap-2.5 rounded-xl border px-4 py-3 text-left text-sm font-medium"
                  style={{
                    backgroundColor: 'var(--accent-amber-soft)',
                    borderColor: 'var(--accent-amber)',
                    color: 'var(--accent-amber)',
                  }}
                >
                  <AlertTriangle className="h-4 w-4 shrink-0" />
                  <span>{nuanceShakeHint}</span>
                </div>
              )}

              {/* Result Banner */}
              {result && (
                <div
                  className="flex w-full max-w-xl items-center justify-between gap-3 rounded-xl px-4 py-3 text-left text-sm"
                  style={{
                    backgroundColor: result.correct
                      ? 'var(--accent-matcha-soft)'
                      : 'var(--accent-vermilion-soft)',
                    color: result.correct ? 'var(--accent-matcha)' : 'var(--accent-vermilion)',
                  }}
                >
                  <div className="flex items-center gap-2.5 font-semibold">
                    {result.correct ? (
                      <CheckCircle2 className="h-5 w-5 shrink-0" />
                    ) : (
                      <XCircle className="h-5 w-5 shrink-0" />
                    )}
                    <span>{result.feedback}</span>
                  </div>
                  {result.alt_answers.length > 0 && (
                    <span className="font-jp text-xs opacity-90">
                      Also valid: {result.alt_answers.slice(0, 3).join(' / ')}
                    </span>
                  )}
                </div>
              )}
            </div>

            {/* Grammar Breakdown Drawer (always shown in Lesson mode, or after answering / hint) */}
            {(result || current.mode === 'lesson' || showHint) && (
              <div
                className="flex flex-col gap-3 rounded-2xl border p-6"
                style={{
                  backgroundColor: 'var(--bg-surface)',
                  borderColor: 'var(--border-subtle)',
                }}
              >
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <div className="font-jp text-base font-bold">
                    {current.title} — <span className="font-sans font-medium">{current.meaning}</span>
                  </div>
                  <div
                    className="font-mono-code rounded-md px-2.5 py-1 text-xs font-semibold"
                    style={{
                      backgroundColor: 'var(--accent-indigo-soft)',
                      color: 'var(--accent-indigo)',
                    }}
                  >
                    {current.structure}
                  </div>
                </div>
                <p className="text-sm leading-relaxed" style={{ color: 'var(--text-secondary)' }}>
                  {current.nuance}
                </p>
                <div
                  className="rounded-xl p-3 text-xs leading-relaxed"
                  style={{
                    backgroundColor: 'var(--bg-subtle)',
                    color: 'var(--text-secondary)',
                  }}
                >
                  <strong style={{ color: 'var(--text-primary)' }}>Nuance & Contrast:</strong>{' '}
                  {current.confusable_with}
                </div>
                {current.nuance_note && (
                  <div className="text-xs" style={{ color: 'var(--text-muted)' }}>
                    Sentence note: {current.nuance_note}
                  </div>
                )}
              </div>
            )}
          </>
        )}
      </div>
    </div>
  )
}

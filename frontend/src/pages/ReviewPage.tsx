import React, { useCallback, useEffect, useRef, useState } from 'react'
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
import { LessonContent } from '../components/LessonContent'
import { useToast } from '../components/ToastContext'
import type {
  FuriganaMode,
  ReviewMode,
  ReviewQueueItem,
  SubmitAnswerResponse,
} from '../types'
import { RubyText } from '../utils/furigana'
import {
  initialLessonStep,
  lessonStepLabel,
  nextLessonStep,
  type LessonStep,
} from '../utils/lessonSteps'
import { applyImeWithCursor, toKanaLive, type ImeMode } from '../utils/romaji'
import { playSentenceAudio } from '../utils/speech'

interface ReviewPageProps {
  initialMode: ReviewMode
  onModeChange?: (mode: ReviewMode) => void
  furiganaMode: FuriganaMode
  onChangeFuriganaMode: (m: FuriganaMode) => void
  knownKanji: Set<string>
  highlightKnown: boolean
  onSessionComplete: () => void
}

const IME_CYCLE: ImeMode[] = ['hiragana', 'katakana', 'off']
const FURIGANA_CYCLE: FuriganaMode[] = ['smart', 'all', 'none']

function getImeLabel(mode: ImeMode): string {
  switch (mode) {
    case 'hiragana':
      return 'あ Hiragana'
    case 'katakana':
      return 'ア Katakana'
    case 'off':
      return 'ABC Off'
  }
}

export const ReviewPage: React.FC<ReviewPageProps> = ({
  initialMode,
  onModeChange,
  furiganaMode,
  onChangeFuriganaMode,
  knownKanji,
  highlightKnown,
  onSessionComplete,
}) => {
  const { toast } = useToast()

  const [mode, setMode] = useState<ReviewMode>(initialMode)
  const [jlptFilter, setJlptFilter] = useState('ALL')
  const [queue, setQueue] = useState<ReviewQueueItem[]>([])
  const [index, setIndex] = useState(0)
  const [loading, setLoading] = useState(true)

  const [inputValue, setInputValue] = useState('')
  const [imeMode, setImeMode] = useState<ImeMode>('hiragana')
  const [showHint, setShowHint] = useState(false)
  const [result, setResult] = useState<SubmitAnswerResponse | null>(null)
  const [nuanceShakeHint, setNuanceShakeHint] = useState<string | null>(null)
  const [nearMissAttempts, setNearMissAttempts] = useState(0)
  const [shaking, setShaking] = useState(false)
  const [wrappingUp, setWrappingUp] = useState(false)
  const [submitting, setSubmitting] = useState(false)
  const [sessionStats, setSessionStats] = useState({ correct: 0, total: 0 })
  const [lessonStep, setLessonStep] = useState<LessonStep>('cloze')
  const [checkPicks, setCheckPicks] = useState<Record<number, number>>({})

  const inputRef = useRef<HTMLInputElement>(null)
  const isComposingRef = useRef(false)

  const selectMode = useCallback(
    (nextMode: ReviewMode) => {
      setMode(nextMode)
      onModeChange?.(nextMode)
    },
    [onModeChange]
  )

  const loadQueue = useCallback(
    async (m: ReviewMode, jlpt: string) => {
      setLoading(true)
      setIndex(0)
      setResult(null)
      setNuanceShakeHint(null)
      setNearMissAttempts(0)
      setWrappingUp(false)
      setInputValue('')
      setShowHint(false)
      setSessionStats({ correct: 0, total: 0 })
      try {
        const items = await api.getReviews(m, jlpt, 20)
        setQueue(items)
      } catch {
        toast.error('Failed to load review queue.')
        setQueue([])
      } finally {
        setLoading(false)
      }
    },
    [toast]
  )

  useEffect(() => {
    setMode(initialMode)
  }, [initialMode])

  useEffect(() => {
    void loadQueue(mode, jlptFilter)
  }, [mode, jlptFilter, loadQueue])

  useEffect(() => {
    if (!result && !loading && lessonStep === 'cloze') {
      inputRef.current?.focus()
    }
  }, [index, result, loading, lessonStep])

  const current = queue[index]
  const hasRichLesson = Boolean(current?.mode === 'lesson' && current.lesson)
  const inLessonPreCloze = hasRichLesson && lessonStep !== 'cloze'
  const checksForStep = current?.lesson?.checks.slice(0, 3) ?? []
  const allChecksAnswered =
    checksForStep.length === 0 ||
    checksForStep.every((_, i) => typeof checkPicks[i] === 'number')

  // Reset Learn New step when the active queue card changes (not on wrap-up slice)
  const activeCardKey = current
    ? `${current.grammar_id}:${current.sentence_id}:${current.mode}:${Boolean(current.lesson)}`
    : ''
  useEffect(() => {
    if (!current) return
    setLessonStep(initialLessonStep(current.mode === 'lesson' && Boolean(current.lesson)))
    setCheckPicks({})
    // activeCardKey encodes the card identity we care about
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [index, activeCardKey])

  const cycleImeMode = () => {
    setImeMode((prev) => IME_CYCLE[(IME_CYCLE.indexOf(prev) + 1) % IME_CYCLE.length])
  }

  const handleInputChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const raw = e.target.value
    const nativeComposing = Boolean((e.nativeEvent as InputEvent | undefined)?.isComposing)
    if (isComposingRef.current || nativeComposing || imeMode === 'off') {
      setInputValue(raw)
      return
    }
    const selectionStart = e.target.selectionStart ?? raw.length
    const { value, cursor } = applyImeWithCursor(raw, selectionStart, imeMode)
    setInputValue(value)
    requestAnimationFrame(() => {
      inputRef.current?.setSelectionRange(cursor, cursor)
    })
  }

  const handleCompositionStart = () => {
    isComposingRef.current = true
  }

  const handleCompositionEnd = (e: React.CompositionEvent<HTMLInputElement>) => {
    isComposingRef.current = false
    const raw = e.currentTarget.value
    if (imeMode === 'off') {
      setInputValue(raw)
      return
    }
    const selectionStart = e.currentTarget.selectionStart ?? raw.length
    const { value, cursor } = applyImeWithCursor(raw, selectionStart, imeMode)
    setInputValue(value)
    requestAnimationFrame(() => {
      inputRef.current?.setSelectionRange(cursor, cursor)
    })
  }

  const goNext = useCallback(() => {
    setResult(null)
    setNuanceShakeHint(null)
    setNearMissAttempts(0)
    setInputValue('')
    setShowHint(false)
    setIndex((i) => i + 1)
  }, [])

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    if (!current || submitting || isComposingRef.current) return

    if (result) {
      goNext()
      return
    }

    const finalInput = toKanaLive(inputValue, imeMode, true)
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
        near_miss_attempts: nearMissAttempts,
      })

      // Non-penalizing "almost right" nuance hint: shake input and let user retry immediately
      if (res.almost_right) {
        setNearMissAttempts((n) => n + 1)
        setNuanceShakeHint(res.nuance_hint || res.feedback)
        setShaking(false)
        requestAnimationFrame(() => setShaking(true))
        window.setTimeout(() => setShaking(false), 400)
        inputRef.current?.focus()
        return
      }

      setNuanceShakeHint(null)
      setNearMissAttempts(0)
      setResult(res)
      setSessionStats((prev) => ({
        correct: prev.correct + (res.correct ? 1 : 0),
        total: prev.total + 1,
      }))
      onSessionComplete()
    } catch {
      toast.error('Failed to submit answer. Please try again.')
    } finally {
      setSubmitting(false)
    }
  }

  const handleWrapUp = () => {
    if (wrappingUp) return
    const cap = Math.min(queue.length, index + 5)
    const remaining = Math.max(0, cap - index)
    setQueue((prev) => prev.slice(0, cap))
    setWrappingUp(true)
    toast.info(`Wrap-up activated: ${remaining} item${remaining === 1 ? '' : 's'} remaining.`)
  }

  const handleUndo = useCallback(async () => {
    if (!current || !result) return
    try {
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
      setNearMissAttempts(0)
      setInputValue('')
      onSessionComplete()
      toast.info('Review undone — give it another try.')
      setTimeout(() => inputRef.current?.focus(), 30)
    } catch {
      toast.error('Could not undo the last review.')
    }
  }, [current, result, onSessionComplete, toast])

  // Global keyboard shortcuts for review ergonomics
  useEffect(() => {
    if (!current || loading) return

    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.isComposing || isComposingRef.current) return

      const target = e.target as HTMLElement | null
      const tag = target?.tagName
      const inInput =
        tag === 'INPUT' ||
        tag === 'TEXTAREA' ||
        tag === 'SELECT' ||
        Boolean(target?.isContentEditable)

      const fullSentence = `${current.jp_before}${current.answer}${current.jp_after}`

      // Audio: Ctrl+Space / Alt+Space anywhere, or bare Space when result is shown or input not focused
      if (e.code === 'Space' || e.key === ' ') {
        const modAudio = (e.ctrlKey || e.altKey) && !e.metaKey
        const bareAudio = !e.ctrlKey && !e.altKey && !e.metaKey && !e.shiftKey && (!inInput || Boolean(result))
        if (modAudio || bareAudio) {
          e.preventDefault()
          void playSentenceAudio(current.sentence_id, fullSentence, 1.0)
          return
        }
      }

      // Hint toggle: Alt+A anywhere, or bare A when not typing in input and before result
      if (e.code === 'KeyA' || e.key.toLowerCase() === 'a') {
        const altA = e.altKey && !e.ctrlKey && !e.metaKey
        const bareA = !e.altKey && !e.ctrlKey && !e.metaKey && !e.shiftKey && !inInput
        if ((altA || bareA) && !result) {
          e.preventDefault()
          setShowHint((h) => !h)
          return
        }
      }

      // Furigana cycle: Alt+F anywhere, or bare F when not typing in input or result is shown
      if (e.code === 'KeyF' || e.key.toLowerCase() === 'f') {
        const altF = e.altKey && !e.ctrlKey && !e.metaKey
        const bareF = !e.altKey && !e.ctrlKey && !e.metaKey && !e.shiftKey && (!inInput || Boolean(result))
        if (altF || bareF) {
          e.preventDefault()
          const nextMode =
            FURIGANA_CYCLE[(FURIGANA_CYCLE.indexOf(furiganaMode) + 1) % FURIGANA_CYCLE.length]
          onChangeFuriganaMode(nextMode)
          return
        }
      }

      // Backspace: Undo & Retry when an incorrect non-cram result is shown
      if (e.key === 'Backspace' && result && !result.correct && current.mode !== 'cram') {
        if (!inInput || (target as HTMLInputElement).disabled) {
          e.preventDefault()
          void handleUndo()
          return
        }
      }

      // Enter: Advance to next card when result is shown (even when input is disabled/blurred)
      if (e.key === 'Enter' && result && !e.metaKey && !e.ctrlKey && !e.altKey) {
        e.preventDefault()
        goNext()
      }
    }

    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [current, loading, result, furiganaMode, onChangeFuriganaMode, handleUndo, goNext])

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
                type="button"
                onClick={() => selectMode(m)}
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
                  type="button"
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
                type="button"
                onClick={() => selectMode('lesson')}
                className="rounded-xl px-5 py-2.5 text-sm font-semibold text-white cursor-pointer"
                style={{ backgroundColor: 'var(--accent-indigo)' }}
              >
                Learn New Grammar
              </button>
              <button
                type="button"
                onClick={() => selectMode('cram')}
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
                    {hasRichLesson
                      ? 'New Lesson — Read, then practice'
                      : 'New Lesson — Study Structure Below, Then Type!'}
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
                    void playSentenceAudio(
                      current.sentence_id,
                      `${current.jp_before}${current.answer}${current.jp_after}`,
                      1.0
                    )
                  }
                  className="flex items-center gap-1.5 rounded-lg border px-3 py-1.5 text-xs font-medium cursor-pointer"
                  style={{
                    backgroundColor: 'var(--bg-surface)',
                    borderColor: 'var(--border-subtle)',
                    color: 'var(--text-secondary)',
                  }}
                  title="Play Japanese Audio (Space / Ctrl+Space)"
                >
                  <Volume2 className="h-3.5 w-3.5" />
                  Listen
                </button>
                <button
                  type="button"
                  onClick={() =>
                    void playSentenceAudio(
                      current.sentence_id,
                      `${current.jp_before}${current.answer}${current.jp_after}`,
                      0.72
                    )
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

            {/* Rich lesson pre-cloze steps (Learn New with lesson payload) */}
            {inLessonPreCloze && current.lesson && (
              <div
                className="flex flex-col gap-6 rounded-2xl border px-5 py-7 sm:px-8"
                style={{
                  backgroundColor: 'var(--bg-surface)',
                  borderColor: 'var(--border-subtle)',
                }}
              >
                <div className="flex flex-wrap items-baseline justify-between gap-2">
                  <h2 className="font-jp text-2xl font-bold">
                    {current.title}{' '}
                    <span className="font-sans text-base font-medium" style={{ color: 'var(--text-secondary)' }}>
                      — {current.lesson.meaning || current.meaning}
                    </span>
                  </h2>
                  <span
                    className="font-mono-code rounded-md px-2.5 py-1 text-xs font-semibold"
                    style={{
                      backgroundColor: 'var(--bg-subtle)',
                      color: 'var(--text-secondary)',
                    }}
                  >
                    {lessonStepLabel(lessonStep)}
                  </span>
                </div>

                {lessonStep === 'read' && (
                  <LessonContent
                    lesson={current.lesson}
                    mode="read"
                    furiganaMode={furiganaMode}
                    knownKanji={knownKanji}
                    highlightKnown={highlightKnown}
                  />
                )}
                {lessonStep === 'examples' && (
                  <LessonContent
                    lesson={current.lesson}
                    mode="examples"
                    exampleLimit={4}
                    furiganaMode={furiganaMode}
                    knownKanji={knownKanji}
                    highlightKnown={highlightKnown}
                  />
                )}
                {lessonStep === 'checks' && (
                  <LessonContent
                    lesson={current.lesson}
                    mode="checks"
                    checkLimit={3}
                    checkPicks={checkPicks}
                    onCheckPick={(checkIndex, optionIndex) =>
                      setCheckPicks((prev) =>
                        typeof prev[checkIndex] === 'number'
                          ? prev
                          : { ...prev, [checkIndex]: optionIndex }
                      )
                    }
                    furiganaMode={furiganaMode}
                    knownKanji={knownKanji}
                    highlightKnown={highlightKnown}
                  />
                )}

                <div className="flex justify-end pt-1">
                  <button
                    type="button"
                    disabled={lessonStep === 'checks' && !allChecksAnswered}
                    onClick={() => {
                      const next = nextLessonStep(lessonStep)
                      if (next) setLessonStep(next)
                    }}
                    className="flex items-center gap-1.5 rounded-lg px-5 py-2.5 text-sm font-semibold text-white cursor-pointer disabled:cursor-not-allowed disabled:opacity-50"
                    style={{ backgroundColor: 'var(--accent-indigo)' }}
                  >
                    Continue <ArrowRight className="h-4 w-4" />
                  </button>
                </div>
              </div>
            )}

            {/* Cloze Question Card */}
            {!inLessonPreCloze && (
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
                    onCompositionStart={handleCompositionStart}
                    onCompositionEnd={handleCompositionEnd}
                    disabled={Boolean(result)}
                    placeholder={
                      imeMode === 'hiragana'
                        ? 'Type romaji (e.g. noni -> のに) or kana...'
                        : imeMode === 'katakana'
                          ? 'Type romaji -> カタカナ...'
                          : 'Type Japanese directly (OS IME)...'
                    }
                    className="font-jp w-full bg-transparent text-xl font-semibold outline-none"
                    style={{ color: 'var(--text-primary)' }}
                    autoComplete="off"
                    spellCheck={false}
                  />

                  <button
                    type="button"
                    onClick={cycleImeMode}
                    className="font-mono-code shrink-0 flex items-center gap-1 rounded-lg px-2.5 py-1.5 text-[11px] font-semibold cursor-pointer"
                    style={{
                      backgroundColor:
                        imeMode === 'hiragana'
                          ? 'var(--accent-matcha-soft)'
                          : imeMode === 'katakana'
                            ? 'var(--accent-indigo-soft)'
                            : 'var(--bg-subtle)',
                      color:
                        imeMode === 'hiragana'
                          ? 'var(--accent-matcha)'
                          : imeMode === 'katakana'
                            ? 'var(--accent-indigo)'
                            : 'var(--text-muted)',
                    }}
                    title="Cycle built-in IME mode: Hiragana -> Katakana -> Off"
                  >
                    <Keyboard className="h-3.5 w-3.5" />
                    {getImeLabel(imeMode)}
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
                        onClick={() => void handleUndo()}
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

              {/* Miss explanation: caution + sentence nuance note (+ lesson mistakes) */}
              {result &&
                !result.correct &&
                (current.caution ||
                  current.nuance_note ||
                  (current.mistakes && current.mistakes.length > 0)) && (
                <div
                  className="w-full max-w-xl rounded-xl border px-4 py-3 text-left text-sm leading-relaxed"
                  style={{
                    backgroundColor: 'var(--bg-subtle)',
                    borderColor: 'var(--border-subtle)',
                    color: 'var(--text-secondary)',
                  }}
                >
                  {current.caution && (
                    <p>
                      <strong style={{ color: 'var(--text-primary)' }}>Watch out:</strong>{' '}
                      {current.caution}
                    </p>
                  )}
                  {current.nuance_note && (
                    <p className={current.caution ? 'mt-1.5' : undefined}>
                      <strong style={{ color: 'var(--text-primary)' }}>Sentence note:</strong>{' '}
                      {current.nuance_note}
                    </p>
                  )}
                  {current.mistakes && current.mistakes.length > 0 && (
                    <div
                      className={
                        current.caution || current.nuance_note ? 'mt-3 flex flex-col gap-2.5' : 'flex flex-col gap-2.5'
                      }
                    >
                      <strong style={{ color: 'var(--text-primary)' }}>Common mistakes:</strong>
                      {current.mistakes.map((m, i) => (
                        <div key={i} className="flex flex-col gap-1">
                          <div className="flex flex-wrap items-start gap-x-3 gap-y-1 font-jp text-base leading-loose">
                            <span style={{ color: 'var(--accent-vermilion)' }}>
                              ✗{' '}
                              <RubyText
                                text={m.wrong_ruby}
                                furiganaMode={furiganaMode}
                                knownKanji={knownKanji}
                                highlightKnown={highlightKnown}
                              />
                            </span>
                            <span style={{ color: 'var(--accent-matcha)' }}>
                              ✓{' '}
                              <RubyText
                                text={m.right_ruby}
                                furiganaMode={furiganaMode}
                                knownKanji={knownKanji}
                                highlightKnown={highlightKnown}
                              />
                            </span>
                          </div>
                          <p className="text-xs leading-relaxed" style={{ color: 'var(--text-muted)' }}>
                            {m.why}
                          </p>
                        </div>
                      ))}
                    </div>
                  )}
                </div>
              )}
            </div>
            )}

            {/* Grammar Breakdown Drawer (lesson without rich content, or after answering / hint) */}
            {(result ||
              showHint ||
              (current.mode === 'lesson' && (!hasRichLesson || lessonStep === 'cloze'))) && (
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
                {current.caution && (
                  <div
                    className="rounded-xl p-3 text-xs leading-relaxed"
                    style={{
                      backgroundColor: 'var(--accent-vermilion-soft)',
                      color: 'var(--text-secondary)',
                    }}
                  >
                    <strong style={{ color: 'var(--accent-vermilion)' }}>Caution:</strong>{' '}
                    {current.caution}
                  </div>
                )}
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

            {/* Keyboard Shortcut Legend Strip */}
            <div
              className="flex flex-wrap items-center justify-center gap-x-4 gap-y-2 rounded-xl border px-4 py-2.5 text-[11px]"
              style={{
                backgroundColor: 'var(--bg-surface)',
                borderColor: 'var(--border-subtle)',
                color: 'var(--text-muted)',
              }}
              aria-label="Keyboard shortcuts"
            >
              <span className="flex items-center gap-1.5">
                <kbd
                  className="font-mono-code rounded border px-1.5 py-0.5 text-[10px] font-semibold"
                  style={{
                    backgroundColor: 'var(--bg-subtle)',
                    borderColor: 'var(--border-subtle)',
                    color: 'var(--text-secondary)',
                  }}
                >
                  Enter
                </kbd>
                Check/Next
              </span>
              <span aria-hidden="true">•</span>
              <span className="flex items-center gap-1.5">
                <kbd
                  className="font-mono-code rounded border px-1.5 py-0.5 text-[10px] font-semibold"
                  style={{
                    backgroundColor: 'var(--bg-subtle)',
                    borderColor: 'var(--border-subtle)',
                    color: 'var(--text-secondary)',
                  }}
                >
                  Space
                </kbd>
                Audio
              </span>
              <span aria-hidden="true">•</span>
              <span className="flex items-center gap-1.5">
                <kbd
                  className="font-mono-code rounded border px-1.5 py-0.5 text-[10px] font-semibold"
                  style={{
                    backgroundColor: 'var(--bg-subtle)',
                    borderColor: 'var(--border-subtle)',
                    color: 'var(--text-secondary)',
                  }}
                >
                  A
                </kbd>
                Hint
              </span>
              <span aria-hidden="true">•</span>
              <span className="flex items-center gap-1.5">
                <kbd
                  className="font-mono-code rounded border px-1.5 py-0.5 text-[10px] font-semibold"
                  style={{
                    backgroundColor: 'var(--bg-subtle)',
                    borderColor: 'var(--border-subtle)',
                    color: 'var(--text-secondary)',
                  }}
                >
                  F
                </kbd>
                Furigana
              </span>
              <span aria-hidden="true">•</span>
              <span className="flex items-center gap-1.5">
                <kbd
                  className="font-mono-code rounded border px-1.5 py-0.5 text-[10px] font-semibold"
                  style={{
                    backgroundColor: 'var(--bg-subtle)',
                    borderColor: 'var(--border-subtle)',
                    color: 'var(--text-secondary)',
                  }}
                >
                  Backspace
                </kbd>
                Undo
              </span>
            </div>
          </>
        )}
      </div>
    </div>
  )
}

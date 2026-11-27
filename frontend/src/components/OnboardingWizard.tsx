import React, { useEffect, useMemo, useState } from 'react'
import {
  ArrowLeft,
  BookOpen,
  Check,
  CheckCircle2,
  Compass,
  Ghost,
  KeyRound,
  Languages,
  LayoutDashboard,
  Moon,
  Sparkles,
  Sun,
} from 'lucide-react'
import { api } from '../api'
import type { AuthUser, DashboardStats, PlacementQuestion, WaniKaniStatus } from '../types'
import {
  DAILY_GOAL_OPTIONS,
  JLPT_LEVEL_ORDER,
  STUDY_PATHS,
  inferPlacementJlpt,
} from '../utils/onboarding'
import { useToast } from './ToastContext'

interface OnboardingWizardProps {
  user: AuthUser
  stats: DashboardStats
  darkMode: boolean
  onToggleDark: () => void
  onCompleted: (nextAction: 'lesson' | 'dashboard') => Promise<void> | void
  onWaniKaniConnected: () => void
}

type WizardStep = 1 | 2 | 3 | 4

const STEP_LABELS: Record<WizardStep, string> = {
  1: 'Study Path',
  2: 'Placement Check',
  3: 'WaniKani Link',
  4: 'Ready to Study',
}

const JLPT_ACCENT: Record<string, { color: string; soft: string; label: string }> = {
  N5: { color: 'var(--accent-matcha)', soft: 'var(--accent-matcha-soft)', label: 'Basic' },
  N4: { color: 'var(--accent-indigo)', soft: 'var(--accent-indigo-soft)', label: 'Elementary' },
  N3: { color: 'var(--accent-amber)', soft: 'var(--accent-amber-soft)', label: 'Intermediate' },
  N2: {
    color: 'var(--accent-vermilion)',
    soft: 'var(--accent-vermilion-soft)',
    label: 'Upper-Int.',
  },
  N1: { color: 'var(--wk-purple)', soft: 'var(--wk-purple-soft)', label: 'Advanced' },
}

export const OnboardingWizard: React.FC<OnboardingWizardProps> = ({
  user,
  stats,
  darkMode,
  onToggleDark,
  onCompleted,
  onWaniKaniConnected,
}) => {
  const { toast } = useToast()

  const initialStudyPath = STUDY_PATHS.some((p) => p.id === stats.onboarding.preferred_study_path)
    ? stats.onboarding.preferred_study_path
    : 'JLPT Core'
  const initialDailyGoal = DAILY_GOAL_OPTIONS.includes(stats.onboarding.daily_goal)
    ? stats.onboarding.daily_goal
    : 20

  const [step, setStep] = useState<WizardStep>(1)
  const [studyPath, setStudyPath] = useState<string>(initialStudyPath)
  const [dailyGoal, setDailyGoal] = useState<number>(initialDailyGoal)

  // Step 2: Placement questions
  const [questions, setQuestions] = useState<PlacementQuestion[]>([])
  const [loadingQuestions, setLoadingQuestions] = useState(true)
  const [questionsError, setQuestionsError] = useState<string | null>(null)
  const [knownIds, setKnownIds] = useState<Set<number>>(new Set())

  // Step 3: Optional WaniKani API v2 connection
  const [wkToken, setWkToken] = useState('')
  const [connectingWk, setConnectingWk] = useState(false)
  const [wkError, setWkError] = useState<string | null>(null)
  const [connectedWk, setConnectedWk] = useState<WaniKaniStatus | null>(null)

  // Final submission state
  const [submitting, setSubmitting] = useState<'lesson' | 'dashboard' | 'skip' | null>(null)

  const loadQuestions = async () => {
    setLoadingQuestions(true)
    setQuestionsError(null)
    try {
      const list = await api.getPlacementQuestions()
      setQuestions(list)
    } catch (err) {
      setQuestionsError(err instanceof Error ? err.message : 'Could not load placement questions.')
    } finally {
      setLoadingQuestions(false)
    }
  }

  useEffect(() => {
    void loadQuestions()
  }, [])

  const inferredJlpt = useMemo(
    () => inferPlacementJlpt(questions, knownIds),
    [questions, knownIds]
  )

  const groupedQuestions = useMemo(() => {
    const map = new Map<string, PlacementQuestion[]>()
    for (const level of JLPT_LEVEL_ORDER) {
      map.set(level, [])
    }
    for (const q of questions) {
      const bucket = map.get(q.jlpt_level)
      if (bucket) {
        bucket.push(q)
      } else {
        map.set(q.jlpt_level, [q])
      }
    }
    return Array.from(map.entries()).filter(([, items]) => items.length > 0)
  }, [questions])

  const effectiveWaniKani: WaniKaniStatus | null = connectedWk?.connected
    ? connectedWk
    : stats.wanikani.connected
      ? stats.wanikani
      : null

  const toggleKnown = (id: number) => {
    setKnownIds((prev) => {
      const next = new Set(prev)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })
  }

  const selectAllInLevel = (items: PlacementQuestion[]) => {
    setKnownIds((prev) => {
      const next = new Set(prev)
      for (const item of items) {
        next.add(item.grammar_id)
      }
      return next
    })
  }

  const clearLevel = (items: PlacementQuestion[]) => {
    setKnownIds((prev) => {
      const next = new Set(prev)
      for (const item of items) {
        next.delete(item.grammar_id)
      }
      return next
    })
  }

  const handleConnectWaniKani = async (e: React.FormEvent) => {
    e.preventDefault()
    const trimmed = wkToken.trim()
    if (!trimmed || connectingWk) return
    setConnectingWk(true)
    setWkError(null)
    try {
      const res = await api.connectWaniKani(trimmed)
      setConnectedWk(res)
      setWkToken('')
      onWaniKaniConnected()
      toast.success(
        `Connected WaniKani as ${res.username || 'learner'} (Level ${res.level})! Background kanji sync started.`
      )
    } catch (err) {
      const msg = err instanceof Error ? err.message : 'Failed to connect WaniKani token.'
      setWkError(msg)
      toast.error(msg)
    } finally {
      setConnectingWk(false)
    }
  }

  const handleSkipSetup = async () => {
    if (submitting) return
    setSubmitting('skip')
    try {
      await api.completeOnboarding({
        study_path: 'JLPT Core',
        placement_jlpt: 'N5',
        daily_goal: 20,
        known_grammar_ids: [],
      })
      toast.info('Setup skipped — starting fresh on JLPT Core (N5).')
      await onCompleted('dashboard')
    } catch (err) {
      const msg = err instanceof Error ? err.message : 'Failed to skip setup.'
      toast.error(msg)
    } finally {
      setSubmitting(null)
    }
  }

  const handleFinish = async (nextAction: 'lesson' | 'dashboard') => {
    if (submitting) return
    setSubmitting(nextAction)
    try {
      await api.completeOnboarding({
        study_path: studyPath,
        placement_jlpt: inferredJlpt,
        daily_goal: dailyGoal,
        known_grammar_ids: Array.from(knownIds),
      })
      toast.success('Study profile ready — がんばって！')
      await onCompleted(nextAction)
    } catch (err) {
      const msg = err instanceof Error ? err.message : 'Failed to save onboarding settings.'
      toast.error(msg)
    } finally {
      setSubmitting(null)
    }
  }

  const busy = submitting !== null

  return (
    <div
      className="min-h-screen flex flex-col transition-colors"
      style={{ backgroundColor: 'var(--bg-canvas)', color: 'var(--text-primary)' }}
    >
      {/* Minimal Top Bar */}
      <header
        className="sticky top-0 z-20 w-full border-b"
        style={{
          backgroundColor: 'var(--bg-surface)',
          borderColor: 'var(--border-subtle)',
        }}
      >
        <div className="mx-auto flex h-[64px] max-w-4xl items-center justify-between gap-4 px-4 sm:px-8">
          <div className="flex items-center gap-3">
            <div
              className="flex h-[34px] w-[34px] items-center justify-center rounded-lg font-jp text-lg font-bold text-white shadow-xs"
              style={{ backgroundColor: 'var(--accent-vermilion)' }}
            >
              文
            </div>
            <div className="flex flex-col text-left">
              <span className="text-[16px] font-bold leading-tight tracking-tight">BunAmateur</span>
              <span
                className="font-mono-code text-[10px] tracking-wider uppercase"
                style={{ color: 'var(--text-muted)' }}
              >
                Setup Wizard
              </span>
            </div>
          </div>

          <div className="hidden sm:flex items-center gap-2">
            <span
              className="font-mono-code rounded-full px-3 py-1 text-xs font-semibold"
              style={{
                backgroundColor: 'var(--bg-subtle)',
                color: 'var(--text-secondary)',
              }}
            >
              Step {step} of 4 · {STEP_LABELS[step]}
            </span>
          </div>

          <div className="flex items-center gap-2.5">
            <button
              type="button"
              disabled={busy}
              onClick={() => void handleSkipSetup()}
              className="rounded-lg px-2.5 py-1.5 text-xs font-semibold transition-opacity hover:opacity-80 disabled:opacity-50 cursor-pointer"
              style={{ color: 'var(--text-muted)' }}
            >
              {submitting === 'skip' ? 'Skipping...' : 'Skip Setup'}
            </button>
            <button
              type="button"
              onClick={onToggleDark}
              className="flex h-9 w-9 items-center justify-center rounded-lg border transition-colors cursor-pointer"
              style={{
                borderColor: 'var(--border-subtle)',
                backgroundColor: 'var(--bg-subtle)',
                color: 'var(--text-secondary)',
              }}
              title={darkMode ? 'Switch to Washi Light Mode' : 'Switch to Sumi Dark Mode'}
              aria-label={darkMode ? 'Switch to Washi Light Mode' : 'Switch to Sumi Dark Mode'}
            >
              {darkMode ? <Sun className="h-4 w-4" /> : <Moon className="h-4 w-4" />}
            </button>
          </div>
        </div>

        {/* 4-segment Progress Rail */}
        <div className="mx-auto max-w-4xl px-4 pb-2.5 sm:px-8">
          <div className="mb-1.5 flex items-center justify-between text-[11px] sm:hidden">
            <span className="font-mono-code font-semibold" style={{ color: 'var(--text-secondary)' }}>
              Step {step} of 4
            </span>
            <span style={{ color: 'var(--text-muted)' }}>{STEP_LABELS[step]}</span>
          </div>
          <div className="grid grid-cols-4 gap-2">
            {([1, 2, 3, 4] as const).map((idx) => (
              <div
                key={idx}
                className="h-1.5 rounded-full transition-colors duration-300"
                style={{
                  backgroundColor:
                    idx < step
                      ? 'var(--accent-matcha)'
                      : idx === step
                        ? 'var(--accent-vermilion)'
                        : 'var(--bg-subtle)',
                }}
              />
            ))}
          </div>
        </div>
      </header>

      {/* Centered Wizard Surface */}
      <main className="mx-auto flex w-full max-w-4xl flex-1 flex-col justify-center px-4 py-8 sm:px-8 sm:py-10">
        <div
          className="flex flex-col gap-6 rounded-2xl border p-6 sm:p-8 shadow-xs"
          style={{
            backgroundColor: 'var(--bg-surface)',
            borderColor: 'var(--border-subtle)',
          }}
        >
          {/* STEP 1 — WELCOME & STUDY PATH */}
          {step === 1 && (
            <>
              <div className="flex flex-col gap-2">
                <div className="flex items-center gap-2">
                  <span
                    className="inline-flex items-center gap-1.5 rounded-full px-2.5 py-0.5 text-[11px] font-semibold"
                    style={{
                      backgroundColor: 'var(--accent-vermilion-soft)',
                      color: 'var(--accent-vermilion)',
                    }}
                  >
                    <Compass className="h-3.5 w-3.5" />
                    Step 1 of 4
                  </span>
                </div>
                <h1 className="font-jp text-2xl font-bold tracking-tight sm:text-3xl">
                  ようこそ, {user.username}!
                </h1>
                <p className="text-sm leading-relaxed" style={{ color: 'var(--text-secondary)' }}>
                  Master Japanese grammar through active fill-in-the-blank recall powered by a{' '}
                  <strong>12-stage SRS</strong> and adaptive{' '}
                  <strong style={{ color: 'var(--accent-vermilion)' }}>Ghost Reviews</strong> that
                  gently drill missed grammar until it sticks.
                </p>
              </div>

              {/* Compact Feature Highlights */}
              <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
                <div
                  className="flex items-start gap-3 rounded-xl border p-3.5"
                  style={{
                    backgroundColor: 'var(--bg-canvas)',
                    borderColor: 'var(--border-subtle)',
                  }}
                >
                  <Sparkles
                    className="mt-0.5 h-4 w-4 shrink-0"
                    style={{ color: 'var(--accent-indigo)' }}
                  />
                  <div>
                    <div className="text-xs font-bold">12-Stage Spaced Repetition</div>
                    <p className="mt-0.5 text-xs" style={{ color: 'var(--text-secondary)' }}>
                      Progress from Apprentice to Burned across N5–N1 with nuance-aware grading.
                    </p>
                  </div>
                </div>
                <div
                  className="flex items-start gap-3 rounded-xl border p-3.5"
                  style={{
                    backgroundColor: 'var(--bg-canvas)',
                    borderColor: 'var(--border-subtle)',
                  }}
                >
                  <Ghost
                    className="mt-0.5 h-4 w-4 shrink-0"
                    style={{ color: 'var(--accent-vermilion)' }}
                  />
                  <div>
                    <div className="text-xs font-bold">Adaptive Ghost Reviews</div>
                    <p className="mt-0.5 text-xs" style={{ color: 'var(--text-secondary)' }}>
                      Missed sentences spawn targeted mini-reviews without derailing your main queue.
                    </p>
                  </div>
                </div>
              </div>

              {/* Study Path Cards */}
              <div className="flex flex-col gap-2.5">
                <div className="flex items-center justify-between">
                  <h2 className="text-sm font-bold">Choose your Study Path</h2>
                  <span className="text-xs" style={{ color: 'var(--text-muted)' }}>
                    Changeable anytime on your dashboard
                  </span>
                </div>
                <div className="grid grid-cols-1 gap-2.5 sm:grid-cols-2 lg:grid-cols-3">
                  {STUDY_PATHS.map((p) => {
                    const active = studyPath === p.id
                    return (
                      <button
                        key={p.id}
                        type="button"
                        onClick={() => setStudyPath(p.id)}
                        className="flex flex-col gap-1.5 rounded-xl border p-4 text-left transition-colors cursor-pointer"
                        style={{
                          backgroundColor: active
                            ? 'var(--accent-indigo-soft)'
                            : 'var(--bg-canvas)',
                          borderColor: active ? 'var(--accent-indigo)' : 'var(--border-subtle)',
                        }}
                      >
                        <div className="flex items-center justify-between gap-2">
                          <span
                            className="text-sm font-bold"
                            style={{
                              color: active ? 'var(--accent-indigo)' : 'var(--text-primary)',
                            }}
                          >
                            {p.label}
                          </span>
                          <span
                            className="font-mono-code rounded-full px-2 py-0.5 text-[10px] font-semibold"
                            style={{
                              backgroundColor: active ? 'var(--accent-indigo)' : 'var(--bg-subtle)',
                              color: active ? '#ffffff' : 'var(--text-secondary)',
                            }}
                          >
                            {p.badge}
                          </span>
                        </div>
                        <span
                          className="text-xs leading-relaxed"
                          style={{ color: 'var(--text-secondary)' }}
                        >
                          {p.desc}
                        </span>
                      </button>
                    )
                  })}
                </div>
              </div>

              {/* Daily Goal Pills */}
              <div className="flex flex-col gap-2.5">
                <h2 className="text-sm font-bold">Daily Review Goal</h2>
                <div className="flex flex-wrap items-center gap-2">
                  {DAILY_GOAL_OPTIONS.map((goal) => {
                    const active = dailyGoal === goal
                    return (
                      <button
                        key={goal}
                        type="button"
                        onClick={() => setDailyGoal(goal)}
                        className="font-mono-code rounded-full border px-4 py-2 text-xs font-semibold transition-colors cursor-pointer"
                        style={{
                          backgroundColor: active
                            ? 'var(--accent-vermilion)'
                            : 'var(--bg-canvas)',
                          borderColor: active ? 'var(--accent-vermilion)' : 'var(--border-subtle)',
                          color: active ? '#ffffff' : 'var(--text-primary)',
                        }}
                      >
                        {goal} / day
                      </button>
                    )
                  })}
                </div>
              </div>

              {/* Step 1 Footer CTA */}
              <div
                className="flex items-center justify-end border-t pt-5"
                style={{ borderColor: 'var(--border-subtle)' }}
              >
                <button
                  type="button"
                  onClick={() => setStep(2)}
                  className="inline-flex items-center gap-2 rounded-xl px-5 py-3 text-sm font-semibold text-white shadow-xs transition-transform active:scale-[0.98] cursor-pointer"
                  style={{ backgroundColor: 'var(--accent-vermilion)' }}
                >
                  <span>Continue to Placement Check →</span>
                </button>
              </div>
            </>
          )}

          {/* STEP 2 — QUICK JLPT PLACEMENT CHECK (N5-N1) */}
          {step === 2 && (
            <>
              <div className="flex flex-wrap items-start justify-between gap-4">
                <div className="flex flex-col gap-1.5">
                  <span
                    className="inline-flex w-fit items-center gap-1.5 rounded-full px-2.5 py-0.5 text-[11px] font-semibold"
                    style={{
                      backgroundColor: 'var(--accent-indigo-soft)',
                      color: 'var(--accent-indigo)',
                    }}
                  >
                    <Sparkles className="h-3.5 w-3.5" />
                    Step 2 of 4 · Quick Placement Check
                  </span>
                  <h1 className="text-xl font-bold tracking-tight sm:text-2xl">
                    Check off grammar you already know
                  </h1>
                  <p className="text-xs leading-relaxed sm:text-sm" style={{ color: 'var(--text-secondary)' }}>
                    Checked points start at <strong>Guru I (4-day interval)</strong> so you can skip
                    basics you have already mastered; unchecked points stay in{' '}
                    <strong>Learn New</strong>.
                  </p>
                </div>

                {/* Live Inferred Starting Level Pill */}
                <div
                  className="font-mono-code inline-flex items-center gap-2 rounded-full border px-3.5 py-1.5 text-xs font-semibold shrink-0"
                  style={{
                    backgroundColor: 'var(--accent-matcha-soft)',
                    borderColor: 'var(--accent-matcha)',
                    color: 'var(--accent-matcha)',
                  }}
                >
                  <span>
                    Starting Level: {inferredJlpt} • {knownIds.size} marked known
                  </span>
                </div>
              </div>

              {loadingQuestions ? (
                <div
                  className="rounded-xl border py-12 text-center text-sm"
                  style={{
                    backgroundColor: 'var(--bg-canvas)',
                    borderColor: 'var(--border-subtle)',
                    color: 'var(--text-muted)',
                  }}
                >
                  Loading JLPT placement grammar...
                </div>
              ) : questionsError ? (
                <div
                  className="flex flex-col items-center gap-3 rounded-xl border p-6 text-center"
                  style={{
                    backgroundColor: 'var(--accent-vermilion-soft)',
                    borderColor: 'var(--accent-vermilion)',
                  }}
                >
                  <p className="text-sm font-medium" style={{ color: 'var(--accent-vermilion)' }}>
                    {questionsError}
                  </p>
                  <button
                    type="button"
                    onClick={() => void loadQuestions()}
                    className="rounded-lg px-4 py-1.5 text-xs font-semibold text-white cursor-pointer"
                    style={{ backgroundColor: 'var(--accent-vermilion)' }}
                  >
                    Retry
                  </button>
                </div>
              ) : (
                <div className="flex max-h-[460px] flex-col gap-5 overflow-y-auto pr-1">
                  {groupedQuestions.map(([level, items]) => {
                    const accent = JLPT_ACCENT[level] ?? {
                      color: 'var(--accent-indigo)',
                      soft: 'var(--accent-indigo-soft)',
                      label: '',
                    }
                    const selectedInLevel = items.filter((q) => knownIds.has(q.grammar_id)).length
                    const allSelected = selectedInLevel === items.length && items.length > 0

                    return (
                      <div key={level} className="flex flex-col gap-2.5">
                        <div className="flex flex-wrap items-center justify-between gap-2">
                          <div className="flex items-center gap-2">
                            <span
                              className="font-mono-code rounded-md px-2 py-0.5 text-xs font-bold"
                              style={{
                                backgroundColor: accent.soft,
                                color: accent.color,
                              }}
                            >
                              {level}
                            </span>
                            {accent.label && (
                              <span
                                className="text-xs font-semibold"
                                style={{ color: 'var(--text-secondary)' }}
                              >
                                {accent.label}
                              </span>
                            )}
                            <span
                              className="font-mono-code text-[11px]"
                              style={{ color: 'var(--text-muted)' }}
                            >
                              ({selectedInLevel}/{items.length})
                            </span>
                          </div>

                          <div className="flex items-center gap-2">
                            {!allSelected && (
                              <button
                                type="button"
                                onClick={() => selectAllInLevel(items)}
                                className="rounded-lg border px-2.5 py-1 text-[11px] font-semibold transition-opacity hover:opacity-80 cursor-pointer"
                                style={{
                                  backgroundColor: 'var(--bg-canvas)',
                                  borderColor: 'var(--border-subtle)',
                                  color: 'var(--text-secondary)',
                                }}
                              >
                                Select all {level}
                              </button>
                            )}
                            {selectedInLevel > 0 && (
                              <button
                                type="button"
                                onClick={() => clearLevel(items)}
                                className="rounded-lg border px-2.5 py-1 text-[11px] font-semibold transition-opacity hover:opacity-80 cursor-pointer"
                                style={{
                                  backgroundColor: 'var(--bg-subtle)',
                                  borderColor: 'var(--border-subtle)',
                                  color: 'var(--text-muted)',
                                }}
                              >
                                Clear
                              </button>
                            )}
                          </div>
                        </div>

                        <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
                          {items.map((q) => {
                            const checked = knownIds.has(q.grammar_id)
                            return (
                              <button
                                key={q.grammar_id}
                                type="button"
                                onClick={() => toggleKnown(q.grammar_id)}
                                className="flex items-start gap-3 rounded-xl border p-3 text-left transition-colors cursor-pointer"
                                style={{
                                  backgroundColor: checked
                                    ? 'var(--accent-matcha-soft)'
                                    : 'var(--bg-canvas)',
                                  borderColor: checked
                                    ? 'var(--accent-matcha)'
                                    : 'var(--border-subtle)',
                                }}
                              >
                                <span
                                  className="mt-0.5 flex h-5 w-5 shrink-0 items-center justify-center rounded-md border"
                                  style={{
                                    backgroundColor: checked
                                      ? 'var(--accent-matcha)'
                                      : 'transparent',
                                    borderColor: checked
                                      ? 'var(--accent-matcha)'
                                      : 'var(--border-subtle)',
                                    color: '#ffffff',
                                  }}
                                >
                                  {checked && <Check className="h-3.5 w-3.5" />}
                                </span>
                                <div className="min-w-0 flex-1">
                                  <div className="flex items-center justify-between gap-2">
                                    <span className="font-jp text-sm font-bold">{q.title}</span>
                                    <span
                                      className="font-mono-code shrink-0 text-[10px]"
                                      style={{ color: 'var(--text-muted)' }}
                                    >
                                      {q.study_path}
                                    </span>
                                  </div>
                                  <div
                                    className="text-xs"
                                    style={{ color: 'var(--text-secondary)' }}
                                  >
                                    {q.meaning}
                                  </div>
                                  <div
                                    className="font-jp mt-1 truncate text-xs"
                                    style={{ color: 'var(--text-muted)' }}
                                  >
                                    {q.sample_jp}
                                  </div>
                                </div>
                              </button>
                            )
                          })}
                        </div>
                      </div>
                    )
                  })}
                </div>
              )}

              {/* Step 2 Footer Navigation */}
              <div
                className="flex flex-wrap items-center justify-between gap-3 border-t pt-5"
                style={{ borderColor: 'var(--border-subtle)' }}
              >
                <button
                  type="button"
                  onClick={() => setStep(1)}
                  className="inline-flex items-center gap-1.5 rounded-xl border px-4 py-2.5 text-xs font-semibold transition-opacity hover:opacity-85 cursor-pointer"
                  style={{
                    borderColor: 'var(--border-subtle)',
                    backgroundColor: 'var(--bg-subtle)',
                    color: 'var(--text-primary)',
                  }}
                >
                  <ArrowLeft className="h-3.5 w-3.5" />
                  Back
                </button>
                <button
                  type="button"
                  onClick={() => setStep(3)}
                  className="inline-flex items-center gap-1.5 rounded-xl px-5 py-2.5 text-sm font-semibold text-white cursor-pointer"
                  style={{ backgroundColor: 'var(--accent-vermilion)' }}
                >
                  <span>Continue →</span>
                </button>
              </div>
            </>
          )}

          {/* STEP 3 — OPTIONAL WANIKANI SMART FURIGANA */}
          {step === 3 && (
            <>
              <div className="flex flex-col gap-1.5">
                <div className="flex items-center gap-2">
                  <span
                    className="inline-flex items-center gap-1.5 rounded-full px-2.5 py-0.5 text-[11px] font-semibold"
                    style={{
                      backgroundColor: 'var(--wk-pink-soft)',
                      color: 'var(--wk-pink)',
                    }}
                  >
                    <Languages className="h-3.5 w-3.5" />
                    Step 3 of 4 · Optional
                  </span>
                </div>
                <h1 className="text-xl font-bold tracking-tight sm:text-2xl">
                  WaniKani Smart Furigana
                </h1>
                <p className="text-sm leading-relaxed" style={{ color: 'var(--text-secondary)' }}>
                  Link a read-only WaniKani API v2 token to automatically hide furigana on kanji you
                  have already reached Guru+ on. Completely optional — you can always connect or
                  adjust this later in <strong>Account Settings → WaniKani</strong>.
                </p>
              </div>

              {effectiveWaniKani ? (
                <div
                  className="flex items-center gap-3.5 rounded-2xl border p-5"
                  style={{
                    backgroundColor: 'var(--accent-matcha-soft)',
                    borderColor: 'var(--accent-matcha)',
                  }}
                >
                  <CheckCircle2
                    className="h-6 w-6 shrink-0"
                    style={{ color: 'var(--accent-matcha)' }}
                  />
                  <div className="flex flex-col gap-0.5">
                    <div
                      className="text-sm font-bold"
                      style={{ color: 'var(--accent-matcha)' }}
                    >
                      Connected as {effectiveWaniKani.username || 'WaniKani user'} (Level{' '}
                      {effectiveWaniKani.level})
                    </div>
                    <p className="text-xs" style={{ color: 'var(--text-secondary)' }}>
                      Smart Furigana is enabled and your known kanji are syncing in the background.
                    </p>
                  </div>
                </div>
              ) : (
                <form
                  onSubmit={handleConnectWaniKani}
                  className="flex flex-col gap-4 rounded-2xl border p-5"
                  style={{
                    backgroundColor: 'var(--bg-canvas)',
                    borderColor: 'var(--border-subtle)',
                  }}
                >
                  <div>
                    <label
                      htmlFor="onboarding-wk-token"
                      className="mb-1.5 block text-xs font-semibold"
                      style={{ color: 'var(--text-secondary)' }}
                    >
                      WaniKani API v2 Personal Access Token (Read-Only)
                    </label>
                    <div className="flex flex-col gap-2.5 sm:flex-row">
                      <div
                        className="flex flex-1 items-center gap-2.5 rounded-xl border px-3.5 py-2.5"
                        style={{
                          backgroundColor: 'var(--bg-surface)',
                          borderColor: 'var(--border-subtle)',
                        }}
                      >
                        <KeyRound
                          className="h-4 w-4 shrink-0"
                          style={{ color: 'var(--text-muted)' }}
                        />
                        <input
                          id="onboarding-wk-token"
                          type="password"
                          value={wkToken}
                          onChange={(e) => {
                            setWkToken(e.target.value)
                            setWkError(null)
                          }}
                          placeholder="Paste your WaniKani API v2 token..."
                          autoComplete="off"
                          className="font-mono-code w-full bg-transparent text-xs outline-none"
                          style={{ color: 'var(--text-primary)' }}
                        />
                      </div>
                      <button
                        type="submit"
                        disabled={connectingWk || !wkToken.trim()}
                        className="shrink-0 rounded-xl px-5 py-2.5 text-xs font-semibold text-white transition-opacity hover:opacity-90 disabled:opacity-50 cursor-pointer"
                        style={{ backgroundColor: 'var(--wk-pink)' }}
                      >
                        {connectingWk ? 'Connecting...' : 'Connect WaniKani'}
                      </button>
                    </div>
                    <p className="mt-2 text-[11px]" style={{ color: 'var(--text-muted)' }}>
                      Generate a read-only token from your WaniKani account settings under{' '}
                      <span className="font-mono-code">API Tokens</span>. BunAmateur never modifies
                      your WaniKani data.
                    </p>
                  </div>

                  {wkError && (
                    <div
                      className="rounded-xl border px-3.5 py-2.5 text-xs font-medium"
                      style={{
                        backgroundColor: 'var(--accent-vermilion-soft)',
                        borderColor: 'var(--accent-vermilion)',
                        color: 'var(--accent-vermilion)',
                      }}
                    >
                      {wkError}
                    </div>
                  )}
                </form>
              )}

              {/* Step 3 Footer Navigation */}
              <div
                className="flex flex-wrap items-center justify-between gap-3 border-t pt-5"
                style={{ borderColor: 'var(--border-subtle)' }}
              >
                <button
                  type="button"
                  onClick={() => setStep(2)}
                  className="inline-flex items-center gap-1.5 rounded-xl border px-4 py-2.5 text-xs font-semibold transition-opacity hover:opacity-85 cursor-pointer"
                  style={{
                    borderColor: 'var(--border-subtle)',
                    backgroundColor: 'var(--bg-subtle)',
                    color: 'var(--text-primary)',
                  }}
                >
                  <ArrowLeft className="h-3.5 w-3.5" />
                  Back
                </button>

                <button
                  type="button"
                  onClick={() => setStep(4)}
                  className="inline-flex items-center gap-1.5 rounded-xl px-5 py-2.5 text-sm font-semibold transition-opacity hover:opacity-90 cursor-pointer"
                  style={
                    effectiveWaniKani
                      ? { backgroundColor: 'var(--accent-vermilion)', color: '#ffffff' }
                      : {
                          backgroundColor: 'var(--accent-indigo)',
                          color: '#ffffff',
                        }
                  }
                >
                  <span>{effectiveWaniKani ? 'Continue →' : 'Skip for now →'}</span>
                </button>
              </div>
            </>
          )}

          {/* STEP 4 — READY TO STUDY */}
          {step === 4 && (
            <>
              <div className="flex flex-col gap-1.5">
                <span
                  className="inline-flex w-fit items-center gap-1.5 rounded-full px-2.5 py-0.5 text-[11px] font-semibold"
                  style={{
                    backgroundColor: 'var(--accent-matcha-soft)',
                    color: 'var(--accent-matcha)',
                  }}
                >
                  <CheckCircle2 className="h-3.5 w-3.5" />
                  Step 4 of 4 · All Set
                </span>
                <h1 className="font-jp text-2xl font-bold tracking-tight sm:text-3xl">
                  準備完了 — Ready to Study!
                </h1>
                <p className="text-sm leading-relaxed" style={{ color: 'var(--text-secondary)' }}>
                  Here is your personalized BunAmateur study plan. Jump straight into your first
                  grammar lesson or head to your dashboard to explore.
                </p>
              </div>

              {/* Summary Card */}
              <div
                className="grid grid-cols-1 gap-3 rounded-2xl border p-5 sm:grid-cols-2"
                style={{
                  backgroundColor: 'var(--bg-canvas)',
                  borderColor: 'var(--border-subtle)',
                }}
              >
                <SummaryItem label="Study Path" value={studyPath} />
                <SummaryItem label="Daily Review Goal" value={`${dailyGoal} reviews / day`} mono />
                <SummaryItem
                  label="Starting JLPT Level"
                  value={inferredJlpt}
                  mono
                  accent="var(--accent-indigo)"
                />
                <SummaryItem
                  label="Grammar Marked Known"
                  value={
                    knownIds.size > 0
                      ? `${knownIds.size} points (starts at Guru I)`
                      : '0 points (fresh start)'
                  }
                  mono
                />
                <div className="sm:col-span-2">
                  <SummaryItem
                    label="WaniKani Smart Furigana"
                    value={
                      effectiveWaniKani
                        ? `Connected as ${effectiveWaniKani.username || 'learner'} (Level ${effectiveWaniKani.level})`
                        : 'Not connected — you can link anytime in Account Settings'
                    }
                    accent={effectiveWaniKani ? 'var(--accent-matcha)' : undefined}
                  />
                </div>
              </div>

              {/* Step 4 Footer CTAs */}
              <div
                className="flex flex-wrap items-center justify-between gap-3 border-t pt-5"
                style={{ borderColor: 'var(--border-subtle)' }}
              >
                <button
                  type="button"
                  disabled={busy}
                  onClick={() => setStep(3)}
                  className="inline-flex items-center gap-1.5 rounded-xl border px-4 py-2.5 text-xs font-semibold transition-opacity hover:opacity-85 disabled:opacity-50 cursor-pointer"
                  style={{
                    borderColor: 'var(--border-subtle)',
                    backgroundColor: 'var(--bg-subtle)',
                    color: 'var(--text-primary)',
                  }}
                >
                  <ArrowLeft className="h-3.5 w-3.5" />
                  Back
                </button>

                <div className="flex flex-wrap items-center gap-2.5">
                  <button
                    type="button"
                    disabled={busy}
                    onClick={() => void handleFinish('dashboard')}
                    className="inline-flex items-center gap-2 rounded-xl border px-4 py-3 text-sm font-semibold transition-opacity hover:opacity-85 disabled:opacity-50 cursor-pointer"
                    style={{
                      borderColor: 'var(--border-subtle)',
                      backgroundColor: 'var(--bg-subtle)',
                      color: 'var(--text-primary)',
                    }}
                  >
                    <LayoutDashboard className="h-4 w-4" />
                    <span>{submitting === 'dashboard' ? 'Saving...' : 'Go to Dashboard'}</span>
                  </button>

                  <button
                    type="button"
                    disabled={busy}
                    onClick={() => void handleFinish('lesson')}
                    className="inline-flex items-center gap-2 rounded-xl px-5 py-3 text-sm font-semibold text-white shadow-xs transition-transform active:scale-[0.98] disabled:opacity-50 cursor-pointer"
                    style={{ backgroundColor: 'var(--accent-vermilion)' }}
                  >
                    <BookOpen className="h-4 w-4" />
                    <span>
                      {submitting === 'lesson' ? 'Starting...' : 'Start First Lesson →'}
                    </span>
                  </button>
                </div>
              </div>
            </>
          )}
        </div>
      </main>
    </div>
  )
}

const SummaryItem: React.FC<{
  label: string
  value: string
  mono?: boolean
  accent?: string
}> = ({ label, value, mono, accent }) => (
  <div
    className="flex flex-col gap-0.5 rounded-xl border px-3.5 py-2.5"
    style={{
      backgroundColor: 'var(--bg-surface)',
      borderColor: 'var(--border-subtle)',
    }}
  >
    <span className="text-[11px] font-semibold" style={{ color: 'var(--text-muted)' }}>
      {label}
    </span>
    <span
      className={`text-sm font-bold ${mono ? 'font-mono-code' : ''}`}
      style={{ color: accent ?? 'var(--text-primary)' }}
    >
      {value}
    </span>
  </div>
)

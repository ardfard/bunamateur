import React, { useEffect, useState } from 'react'
import { Check, Compass, Sparkles } from 'lucide-react'
import { api } from '../api'
import type { PlacementQuestion } from '../types'

interface OnboardingPanelProps {
  onCompleted: () => void
}

const STUDY_PATHS = [
  { id: 'JLPT Core', label: 'JLPT Core (N5 → N1)', desc: 'Balanced JLPT progression across all levels' },
  { id: 'Genki I', label: 'Genki I', desc: 'Beginner essentials (JLPT N5)' },
  { id: 'Genki II', label: 'Genki II', desc: 'Elementary structures (JLPT N4)' },
  { id: 'Tobira', label: 'Tobira', desc: 'Intermediate bridge (JLPT N3)' },
  { id: 'Quartet', label: 'Quartet', desc: 'Upper-intermediate reading & writing (JLPT N2)' },
]

export const OnboardingPanel: React.FC<OnboardingPanelProps> = ({ onCompleted }) => {
  const [step, setStep] = useState<'path' | 'placement'>('path')
  const [studyPath, setStudyPath] = useState('JLPT Core')
  const [dailyGoal, setDailyGoal] = useState(20)
  const [questions, setQuestions] = useState<PlacementQuestion[]>([])
  const [knownIds, setKnownIds] = useState<Set<number>>(new Set())
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    api
      .getPlacementQuestions()
      .then(setQuestions)
      .catch(() => {})
  }, [])

  const toggleKnown = (id: number) => {
    setKnownIds((prev) => {
      const next = new Set(prev)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })
  }

  const inferPlacementJlpt = (): string => {
    const knownQuestions = questions.filter((q) => knownIds.has(q.grammar_id))
    const levels = new Set(knownQuestions.map((q) => q.jlpt_level))
    if (levels.has('N2')) return 'N2'
    if (levels.has('N3')) return 'N3'
    if (levels.has('N4')) return 'N4'
    return 'N5'
  }

  const submitOnboarding = async (skipPlacement: boolean) => {
    setBusy(true)
    try {
      await api.completeOnboarding({
        study_path: studyPath,
        placement_jlpt: skipPlacement ? 'N5' : inferPlacementJlpt(),
        daily_goal: dailyGoal,
        known_grammar_ids: skipPlacement ? [] : Array.from(knownIds),
      })
      onCompleted()
    } finally {
      setBusy(false)
    }
  }

  return (
    <section
      className="flex flex-col gap-5 rounded-2xl border-2 p-6"
      style={{
        backgroundColor: 'var(--bg-surface)',
        borderColor: 'var(--accent-indigo)',
      }}
    >
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="flex items-center gap-2.5">
          <div
            className="flex h-9 w-9 items-center justify-center rounded-xl text-white"
            style={{ backgroundColor: 'var(--accent-indigo)' }}
          >
            <Compass className="h-5 w-5" />
          </div>
          <div>
            <h2 className="text-base font-bold">Welcome to BunAmateur — Set Up Your Study Path</h2>
            <p className="text-xs" style={{ color: 'var(--text-secondary)' }}>
              Clean zero-state start: choose your textbook deck and optionally check off grammar you already know.
            </p>
          </div>
        </div>
        <button
          type="button"
          disabled={busy}
          onClick={() => submitOnboarding(true)}
          className="text-xs font-semibold hover:underline cursor-pointer"
          style={{ color: 'var(--text-muted)' }}
        >
          Skip &amp; Start from N5 Zero-State
        </button>
      </div>

      {step === 'path' ? (
        <>
          <div className="grid grid-cols-1 gap-2.5 sm:grid-cols-2 lg:grid-cols-5">
            {STUDY_PATHS.map((p) => {
              const active = studyPath === p.id
              return (
                <button
                  key={p.id}
                  type="button"
                  onClick={() => setStudyPath(p.id)}
                  className="flex flex-col gap-1 rounded-xl border p-3.5 text-left transition-colors cursor-pointer"
                  style={{
                    backgroundColor: active ? 'var(--accent-indigo-soft)' : 'var(--bg-canvas)',
                    borderColor: active ? 'var(--accent-indigo)' : 'var(--border-subtle)',
                  }}
                >
                  <span className="text-xs font-bold" style={{ color: active ? 'var(--accent-indigo)' : 'var(--text-primary)' }}>
                    {p.label}
                  </span>
                  <span className="text-[11px]" style={{ color: 'var(--text-secondary)' }}>
                    {p.desc}
                  </span>
                </button>
              )
            })}
          </div>

          <div className="flex flex-wrap items-center justify-between gap-4 pt-1">
            <label className="flex items-center gap-2.5 text-xs font-medium">
              <span style={{ color: 'var(--text-secondary)' }}>Daily review goal:</span>
              <select
                value={dailyGoal}
                onChange={(e) => setDailyGoal(Number(e.target.value))}
                className="font-mono-code rounded-lg border px-2.5 py-1 text-xs outline-none"
                style={{
                  backgroundColor: 'var(--bg-canvas)',
                  borderColor: 'var(--border-subtle)',
                  color: 'var(--text-primary)',
                }}
              >
                {[10, 15, 20, 30, 50].map((n) => (
                  <option key={n} value={n}>
                    {n} reviews / day
                  </option>
                ))}
              </select>
            </label>

            <div className="flex items-center gap-2.5">
              <button
                type="button"
                disabled={busy}
                onClick={() => setStep('placement')}
                className="flex items-center gap-1.5 rounded-xl px-4 py-2 text-xs font-semibold text-white cursor-pointer"
                style={{ backgroundColor: 'var(--accent-indigo)' }}
              >
                <Sparkles className="h-3.5 w-3.5" />
                Quick Placement Check →
              </button>
              <button
                type="button"
                disabled={busy}
                onClick={() => submitOnboarding(true)}
                className="rounded-xl border px-4 py-2 text-xs font-semibold cursor-pointer"
                style={{
                  borderColor: 'var(--border-subtle)',
                  backgroundColor: 'var(--bg-subtle)',
                  color: 'var(--text-primary)',
                }}
              >
                Save Path &amp; Start
              </button>
            </div>
          </div>
        </>
      ) : (
        <>
          <p className="text-xs" style={{ color: 'var(--text-secondary)' }}>
            Check any grammar points you already know comfortably. Checked points start at <strong>Guru I</strong> (4-day interval); unchecked points stay in your New Lessons queue.
          </p>
          <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
            {questions.map((q) => {
              const checked = knownIds.has(q.grammar_id)
              return (
                <button
                  key={q.grammar_id}
                  type="button"
                  onClick={() => toggleKnown(q.grammar_id)}
                  className="flex items-start gap-3 rounded-xl border p-3 text-left transition-colors cursor-pointer"
                  style={{
                    backgroundColor: checked ? 'var(--accent-matcha-soft)' : 'var(--bg-canvas)',
                    borderColor: checked ? 'var(--accent-matcha)' : 'var(--border-subtle)',
                  }}
                >
                  <span
                    className="mt-0.5 flex h-5 w-5 shrink-0 items-center justify-center rounded-md border"
                    style={{
                      backgroundColor: checked ? 'var(--accent-matcha)' : 'transparent',
                      borderColor: checked ? 'var(--accent-matcha)' : 'var(--border-subtle)',
                      color: '#ffffff',
                    }}
                  >
                    {checked && <Check className="h-3.5 w-3.5" />}
                  </span>
                  <div className="min-w-0">
                    <div className="flex items-center gap-2">
                      <span className="font-jp text-sm font-bold">{q.title}</span>
                      <span className="font-mono-code text-[10px] font-semibold" style={{ color: 'var(--text-muted)' }}>
                        {q.jlpt_level} • {q.study_path}
                      </span>
                    </div>
                    <div className="text-xs" style={{ color: 'var(--text-secondary)' }}>
                      {q.meaning}
                    </div>
                    <div className="font-jp mt-1 truncate text-xs" style={{ color: 'var(--text-muted)' }}>
                      {q.sample_jp}
                    </div>
                  </div>
                </button>
              )
            })}
          </div>

          <div className="flex flex-wrap items-center justify-between gap-3 pt-1">
            <button
              type="button"
              onClick={() => setStep('path')}
              className="text-xs font-semibold hover:underline cursor-pointer"
              style={{ color: 'var(--text-secondary)' }}
            >
              ← Back to Study Path
            </button>
            <button
              type="button"
              disabled={busy}
              onClick={() => submitOnboarding(false)}
              className="rounded-xl px-5 py-2 text-xs font-semibold text-white cursor-pointer disabled:opacity-50"
              style={{ backgroundColor: 'var(--accent-matcha)' }}
            >
              {busy
                ? 'Saving...'
                : `Finish Onboarding (${knownIds.size} marked known • ${inferPlacementJlpt()})`}
            </button>
          </div>
        </>
      )}
    </section>
  )
}

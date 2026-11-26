import React, { useEffect, useRef, useState } from 'react'
import { ArrowRight, CheckCircle2, Flame, Keyboard, Swords, Volume2, XCircle } from 'lucide-react'
import { api } from '../api'
import type { ConjugationItem } from '../types'
import { applyImeWithCursor, toKanaLive, type ImeMode } from '../utils/romaji'
import { speakJapanese } from '../utils/speech'

const FORM_FILTERS = [
  ['all', 'All Forms'],
  ['te', 'て-form'],
  ['nai', 'ない-form'],
  ['ta', 'た-form'],
  ['potential', 'Potential'],
  ['ba', 'ば-Conditional'],
  ['passive', 'Passive'],
  ['causative', 'Causative'],
]

const IME_CYCLE: ImeMode[] = ['hiragana', 'katakana', 'off']

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

export const ConjugationPage: React.FC = () => {
  const [formFilter, setFormFilter] = useState('all')
  const [items, setItems] = useState<ConjugationItem[]>([])
  const [idx, setIdx] = useState(0)
  const [input, setInput] = useState('')
  const [imeMode, setImeMode] = useState<ImeMode>('hiragana')
  const [checked, setChecked] = useState<null | { correct: boolean }>(null)
  const [streak, setStreak] = useState(0)
  const [bestStreak, setBestStreak] = useState(0)
  const inputRef = useRef<HTMLInputElement>(null)
  const isComposingRef = useRef(false)

  useEffect(() => {
    api.getConjugations(formFilter).then((list) => {
      // Shuffle lightly for fresh drilling
      const shuffled = [...list].sort(() => Math.random() - 0.5)
      setItems(shuffled)
      setIdx(0)
      setInput('')
      setChecked(null)
    })
  }, [formFilter])

  useEffect(() => {
    if (!checked) inputRef.current?.focus()
  }, [idx, checked])

  const current = items[idx % Math.max(1, items.length)]

  const cycleImeMode = () => {
    setImeMode((prev) => IME_CYCLE[(IME_CYCLE.indexOf(prev) + 1) % IME_CYCLE.length])
  }

  const handleInputChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const raw = e.target.value
    const nativeComposing = Boolean((e.nativeEvent as InputEvent | undefined)?.isComposing)
    if (isComposingRef.current || nativeComposing || imeMode === 'off') {
      setInput(raw)
      return
    }
    const selectionStart = e.target.selectionStart ?? raw.length
    const { value, cursor } = applyImeWithCursor(raw, selectionStart, imeMode)
    setInput(value)
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
      setInput(raw)
      return
    }
    const selectionStart = e.currentTarget.selectionStart ?? raw.length
    const { value, cursor } = applyImeWithCursor(raw, selectionStart, imeMode)
    setInput(value)
    requestAnimationFrame(() => {
      inputRef.current?.setSelectionRange(cursor, cursor)
    })
  }

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault()
    if (!current || isComposingRef.current) return
    if (checked) {
      setChecked(null)
      setInput('')
      setIdx((i) => i + 1)
      return
    }
    const finalAns = toKanaLive(input, imeMode, true).trim()
    if (!finalAns) return
    setInput(finalAns)

    const correct =
      finalAns === current.answer_kana || finalAns === current.answer_kanji
    setChecked({ correct })
    if (correct) {
      setStreak((s) => {
        const next = s + 1
        setBestStreak((b) => Math.max(b, next))
        return next
      })
    } else {
      setStreak(0)
    }
  }

  return (
    <div className="mx-auto flex max-w-[920px] flex-col gap-6 px-4 py-8 sm:px-6">
      <div className="flex flex-wrap items-end justify-between gap-4">
        <div>
          <div className="flex items-center gap-2">
            <Swords className="h-6 w-6" style={{ color: 'var(--accent-vermilion)' }} />
            <h1 className="font-jp text-2xl font-bold tracking-tight">
              活用道場 — Conjugation Dojo
            </h1>
          </div>
          <p className="mt-1 text-sm" style={{ color: 'var(--text-secondary)' }}>
            Build instant reflex speed on Japanese verb & adjective conjugations with live Romaji-to-Kana IME.
          </p>
        </div>

        <div className="flex items-center gap-2.5">
          <div
            className="flex items-center gap-1.5 rounded-full px-3.5 py-1.5 font-mono-code text-xs font-bold"
            style={{
              backgroundColor: 'var(--accent-amber-soft)',
              color: 'var(--accent-amber)',
            }}
          >
            <Flame className="h-3.5 w-3.5" />
            Streak: {streak} (Best: {bestStreak})
          </div>
        </div>
      </div>

      {/* Form Filter Pills */}
      <div className="flex flex-wrap gap-1.5">
        {FORM_FILTERS.map(([val, lbl]) => (
          <button
            key={val}
            type="button"
            onClick={() => setFormFilter(val)}
            className="rounded-full border px-3.5 py-1.5 text-xs font-semibold transition-colors cursor-pointer"
            style={{
              backgroundColor: formFilter === val ? 'var(--accent-vermilion)' : 'var(--bg-surface)',
              borderColor: formFilter === val ? 'var(--accent-vermilion)' : 'var(--border-subtle)',
              color: formFilter === val ? '#ffffff' : 'var(--text-secondary)',
            }}
          >
            {lbl}
          </button>
        ))}
      </div>

      {/* Drill Card */}
      {current && (
        <div
          className="flex flex-col items-center gap-6 rounded-2xl border px-6 py-10 text-center sm:px-12"
          style={{
            backgroundColor: 'var(--bg-surface)',
            borderColor: 'var(--border-subtle)',
          }}
        >
          <div className="flex items-center gap-2">
            <span
              className="font-mono-code rounded-full px-3 py-1 text-xs font-bold uppercase"
              style={{
                backgroundColor: 'var(--bg-subtle)',
                color: 'var(--text-secondary)',
              }}
            >
              {current.word_type}
            </span>
            <span
              className="rounded-full px-3 py-1 text-xs font-bold"
              style={{
                backgroundColor: 'var(--accent-indigo-soft)',
                color: 'var(--accent-indigo)',
              }}
            >
              Target: {current.target_label}
            </span>
          </div>

          <div className="flex flex-col items-center gap-1">
            <div className="font-jp text-5xl font-bold">{current.dictionary}</div>
            <div className="font-jp text-base" style={{ color: 'var(--text-muted)' }}>
              {current.reading} • &ldquo;{current.meaning}&rdquo;
            </div>
          </div>

          <form onSubmit={handleSubmit} className="w-full max-w-lg">
            <div
              className="flex items-center gap-2 rounded-xl border-2 px-4 py-2.5"
              style={{
                backgroundColor: 'var(--bg-canvas)',
                borderColor: checked
                  ? checked.correct
                    ? 'var(--accent-matcha)'
                    : 'var(--accent-vermilion)'
                  : 'var(--border-subtle)',
              }}
            >
              <input
                ref={inputRef}
                type="text"
                value={input}
                onChange={handleInputChange}
                onCompositionStart={handleCompositionStart}
                onCompositionEnd={handleCompositionEnd}
                disabled={Boolean(checked)}
                placeholder={
                  imeMode === 'hiragana'
                    ? 'Type conjugated form (romaji -> kana)...'
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
                className="flex shrink-0 items-center gap-1 rounded-lg px-4 py-2 text-xs font-semibold text-white cursor-pointer"
                style={{
                  backgroundColor: checked
                    ? checked.correct
                      ? 'var(--accent-matcha)'
                      : 'var(--accent-vermilion)'
                    : 'var(--accent-vermilion)',
                }}
              >
                {checked ? (
                  <>
                    Next <ArrowRight className="h-3.5 w-3.5" />
                  </>
                ) : (
                  'Check'
                )}
              </button>
            </div>
          </form>

          {checked && (
            <div className="flex w-full max-w-xl flex-col gap-3 text-left">
              <div
                className="flex items-center justify-between rounded-xl px-4 py-3 text-sm font-semibold"
                style={{
                  backgroundColor: checked.correct
                    ? 'var(--accent-matcha-soft)'
                    : 'var(--accent-vermilion-soft)',
                  color: checked.correct ? 'var(--accent-matcha)' : 'var(--accent-vermilion)',
                }}
              >
                <div className="flex items-center gap-2">
                  {checked.correct ? (
                    <CheckCircle2 className="h-5 w-5" />
                  ) : (
                    <XCircle className="h-5 w-5" />
                  )}
                  <span>
                    {checked.correct
                      ? `Spot on! ${current.answer_kanji} (${current.answer_kana})`
                      : `Answer: ${current.answer_kanji} (${current.answer_kana})`}
                  </span>
                </div>
                <button
                  type="button"
                  onClick={() => speakJapanese(current.answer_kanji)}
                  className="rounded-lg p-1.5 hover:opacity-75 cursor-pointer"
                >
                  <Volume2 className="h-4 w-4" />
                </button>
              </div>

              <div
                className="rounded-xl border p-4 text-sm"
                style={{
                  backgroundColor: 'var(--bg-canvas)',
                  borderColor: 'var(--border-subtle)',
                }}
              >
                <div className="text-xs font-bold uppercase tracking-wider" style={{ color: 'var(--text-muted)' }}>
                  Conjugation Rule
                </div>
                <p className="mt-1 leading-relaxed">{current.rule_explanation}</p>
                <div
                  className="mt-3 flex items-center justify-between border-t pt-3"
                  style={{ borderColor: 'var(--border-subtle)' }}
                >
                  <div>
                    <div className="font-jp font-semibold">{current.example_jp}</div>
                    <div className="text-xs" style={{ color: 'var(--text-secondary)' }}>
                      {current.example_en}
                    </div>
                  </div>
                  <button
                    type="button"
                    onClick={() => speakJapanese(current.example_jp)}
                    className="rounded-lg p-2 cursor-pointer"
                    style={{ backgroundColor: 'var(--bg-surface)', color: 'var(--accent-indigo)' }}
                  >
                    <Volume2 className="h-4 w-4" />
                  </button>
                </div>
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  )
}

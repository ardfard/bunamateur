import React, { useEffect, useState } from 'react'
import { BookMarked, Sparkles, Volume2, Wand2 } from 'lucide-react'
import { api } from '../api'
import type { FuriganaMode, ReadingAnalyzeResponse, ReadingPassage } from '../types'
import { RubyText } from '../utils/furigana'
import { speakJapanese } from '../utils/speech'

interface ReadingLabPageProps {
  furiganaMode: FuriganaMode
  knownKanji: Set<string>
  highlightKnown: boolean
  onSelectGrammar: (id: number) => void
}

export const ReadingLabPage: React.FC<ReadingLabPageProps> = ({
  furiganaMode,
  knownKanji,
  highlightKnown,
  onSelectGrammar,
}) => {
  const [passages, setPassages] = useState<ReadingPassage[]>([])
  const [selectedId, setSelectedId] = useState<number | null>(null)
  const [showEnglish, setShowEnglish] = useState(true)

  const [customText, setCustomText] = useState(
    '毎日日本語を勉強しているおかげで、少しずつ日本の新聞が読めるようになりました。雨にもかかわらず、公園へ行くつもりです。'
  )
  const [analysis, setAnalysis] = useState<ReadingAnalyzeResponse | null>(null)
  const [analyzing, setAnalyzing] = useState(false)

  useEffect(() => {
    api.getReadingPassages().then((list) => {
      setPassages(list)
      if (list.length > 0) setSelectedId(list[0].id)
    })
  }, [])

  const activePassage = passages.find((p) => p.id === selectedId) || passages[0]

  const handleAnalyze = async () => {
    if (!customText.trim()) return
    setAnalyzing(true)
    try {
      const res = await api.analyzeReading(customText)
      setAnalysis(res)
    } finally {
      setAnalyzing(false)
    }
  }

  return (
    <div className="mx-auto flex max-w-[1360px] flex-col gap-6 px-4 py-8 sm:px-8">
      <div>
        <div className="flex items-center gap-2">
          <BookMarked className="h-6 w-6" style={{ color: 'var(--accent-matcha)' }} />
          <h1 className="font-jp text-2xl font-bold tracking-tight">
            読解ラボ — Immersion Reading Lab
          </h1>
        </div>
        <p className="mt-1 text-sm" style={{ color: 'var(--text-secondary)' }}>
          Read graded Japanese passages with WaniKani Smart Furigana, or paste any Japanese text to detect JLPT grammar & unknown kanji.
        </p>
      </div>

      <div className="grid grid-cols-1 gap-6 lg:grid-cols-12">
        {/* Left: Curated Graded Passages */}
        <div className="flex flex-col gap-4 lg:col-span-7">
          <div className="flex flex-wrap gap-2">
            {passages.map((p) => {
              const active = activePassage?.id === p.id
              return (
                <button
                  key={p.id}
                  onClick={() => setSelectedId(p.id)}
                  className="flex items-center gap-2 rounded-xl border px-3.5 py-2 text-xs font-semibold transition-colors cursor-pointer"
                  style={{
                    backgroundColor: active ? 'var(--accent-vermilion)' : 'var(--bg-surface)',
                    borderColor: active ? 'var(--accent-vermilion)' : 'var(--border-subtle)',
                    color: active ? '#ffffff' : 'var(--text-secondary)',
                  }}
                >
                  <span className="font-mono-code opacity-85">{p.jlpt_level}</span>
                  <span>{p.topic}</span>
                </button>
              )
            })}
          </div>

          {activePassage && (
            <div
              className="flex flex-col gap-5 rounded-2xl border p-6"
              style={{
                backgroundColor: 'var(--bg-surface)',
                borderColor: 'var(--border-subtle)',
              }}
            >
              <div className="flex flex-wrap items-start justify-between gap-3">
                <div>
                  <div className="flex items-center gap-2">
                    <span
                      className="font-mono-code rounded-md px-2 py-0.5 text-xs font-bold"
                      style={{
                        backgroundColor: 'var(--accent-indigo-soft)',
                        color: 'var(--accent-indigo)',
                      }}
                    >
                      {activePassage.jlpt_level}
                    </span>
                    <span
                      className="font-mono-code rounded-md px-2 py-0.5 text-xs font-bold"
                      style={{
                        backgroundColor: 'var(--accent-matcha-soft)',
                        color: 'var(--accent-matcha)',
                      }}
                    >
                      {activePassage.wk_readiness_pct}% WK Readable
                    </span>
                  </div>
                  <h2 className="font-jp mt-2 text-lg font-bold">{activePassage.title}</h2>
                </div>

                <div className="flex items-center gap-2">
                  <button
                    onClick={() => speakJapanese(activePassage.content_ruby, 0.88)}
                    className="flex items-center gap-1.5 rounded-lg border px-3 py-1.5 text-xs font-semibold cursor-pointer"
                    style={{
                      backgroundColor: 'var(--bg-subtle)',
                      borderColor: 'var(--border-subtle)',
                      color: 'var(--text-primary)',
                    }}
                  >
                    <Volume2 className="h-3.5 w-3.5" />
                    Read Aloud
                  </button>
                  <button
                    onClick={() => setShowEnglish((v) => !v)}
                    className="rounded-lg border px-3 py-1.5 text-xs font-semibold cursor-pointer"
                    style={{
                      borderColor: 'var(--border-subtle)',
                      color: 'var(--text-secondary)',
                    }}
                  >
                    {showEnglish ? 'Hide EN' : 'Show EN'}
                  </button>
                </div>
              </div>

              <div
                className="rounded-xl border p-5 font-jp text-lg leading-[2.3]"
                style={{
                  backgroundColor: 'var(--bg-canvas)',
                  borderColor: 'var(--border-subtle)',
                }}
              >
                <RubyText
                  text={activePassage.content_ruby}
                  furiganaMode={furiganaMode}
                  knownKanji={knownKanji}
                  highlightKnown={highlightKnown}
                />
              </div>

              {showEnglish && (
                <p className="text-sm leading-relaxed" style={{ color: 'var(--text-secondary)' }}>
                  {activePassage.english}
                </p>
              )}

              {/* Detected Grammar Chips */}
              <div className="flex flex-col gap-2.5">
                <div className="text-xs font-bold uppercase tracking-wider" style={{ color: 'var(--text-muted)' }}>
                  Detected Grammar Points ({activePassage.detected_grammar.length}) — Click to Inspect
                </div>
                <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
                  {activePassage.detected_grammar.map((dg) => (
                    <button
                      key={dg.grammar_id}
                      onClick={() => onSelectGrammar(dg.grammar_id)}
                      className="flex items-center justify-between rounded-xl border px-3.5 py-2.5 text-left transition-colors hover:opacity-85 cursor-pointer"
                      style={{
                        backgroundColor: 'var(--bg-subtle)',
                        borderColor: 'var(--border-subtle)',
                      }}
                    >
                      <div>
                        <div className="font-jp text-sm font-bold">
                          {dg.title}{' '}
                          <span className="font-mono-code text-[11px] font-semibold" style={{ color: 'var(--accent-indigo)' }}>
                            ({dg.jlpt_level})
                          </span>
                        </div>
                        <div className="text-xs" style={{ color: 'var(--text-secondary)' }}>
                          {dg.meaning}
                        </div>
                      </div>
                      <span
                        className="font-mono-code shrink-0 text-[11px] font-semibold"
                        style={{
                          color: dg.srs_stage > 0 ? 'var(--accent-matcha)' : 'var(--accent-vermilion)',
                        }}
                      >
                        {dg.srs_label}
                      </span>
                    </button>
                  ))}
                </div>
              </div>
            </div>
          )}
        </div>

        {/* Right: Custom Japanese Text Analyzer */}
        <div
          className="flex flex-col gap-4 rounded-2xl border p-6 lg:col-span-5"
          style={{
            backgroundColor: 'var(--bg-surface)',
            borderColor: 'var(--border-subtle)',
          }}
        >
          <div className="flex items-center justify-between">
            <h2 className="flex items-center gap-2 text-base font-bold">
              <Wand2 className="h-4 w-4" style={{ color: 'var(--wk-pink)' }} />
              Analyze Your Own Japanese Text
            </h2>
          </div>
          <p className="text-xs leading-relaxed" style={{ color: 'var(--text-secondary)' }}>
            Paste a news paragraph, manga dialogue, or diary entry. BunAmateur scans it for JLPT grammar points and checks every kanji against your WaniKani profile.
          </p>

          <textarea
            rows={5}
            value={customText}
            onChange={(e) => setCustomText(e.target.value)}
            className="font-jp w-full rounded-xl border p-3.5 text-sm leading-relaxed outline-none"
            style={{
              backgroundColor: 'var(--bg-canvas)',
              borderColor: 'var(--border-subtle)',
              color: 'var(--text-primary)',
            }}
            placeholder="Paste Japanese text here..."
          />

          <button
            onClick={handleAnalyze}
            disabled={analyzing}
            className="flex items-center justify-center gap-2 rounded-xl px-4 py-2.5 text-sm font-semibold text-white cursor-pointer"
            style={{ backgroundColor: 'var(--accent-indigo)' }}
          >
            <Sparkles className="h-4 w-4" />
            {analyzing ? 'Analyzing...' : 'Analyze Grammar & WK Kanji'}
          </button>

          {analysis && (
            <div className="flex flex-col gap-4 pt-2">
              <div className="grid grid-cols-3 gap-2.5">
                <div className="rounded-xl p-3" style={{ backgroundColor: 'var(--accent-matcha-soft)' }}>
                  <div className="font-mono-code text-lg font-bold" style={{ color: 'var(--accent-matcha)' }}>
                    {analysis.wk_readiness_pct}%
                  </div>
                  <div className="text-[11px]" style={{ color: 'var(--text-secondary)' }}>
                    WK Readable
                  </div>
                </div>
                <div className="rounded-xl p-3" style={{ backgroundColor: 'var(--wk-pink-soft)' }}>
                  <div className="font-mono-code text-lg font-bold" style={{ color: 'var(--wk-pink)' }}>
                    {analysis.known_kanji}/{analysis.total_kanji}
                  </div>
                  <div className="text-[11px]" style={{ color: 'var(--text-secondary)' }}>
                    Kanji Known
                  </div>
                </div>
                <div className="rounded-xl p-3" style={{ backgroundColor: 'var(--accent-indigo-soft)' }}>
                  <div className="font-mono-code text-lg font-bold" style={{ color: 'var(--accent-indigo)' }}>
                    {analysis.detected_grammar.length}
                  </div>
                  <div className="text-[11px]" style={{ color: 'var(--text-secondary)' }}>
                    Grammar Hits
                  </div>
                </div>
              </div>

              {analysis.unknown_kanji.length > 0 && (
                <div>
                  <div className="mb-1.5 text-xs font-bold" style={{ color: 'var(--text-muted)' }}>
                    Kanji Not Yet Passed in WaniKani ({analysis.unknown_kanji.length})
                  </div>
                  <div className="flex flex-wrap gap-1.5">
                    {analysis.unknown_kanji.map((k) => (
                      <span
                        key={k}
                        className="font-jp rounded-lg border px-2.5 py-1 text-base font-bold"
                        style={{
                          backgroundColor: 'var(--bg-canvas)',
                          borderColor: 'var(--border-subtle)',
                        }}
                      >
                        {k}
                      </span>
                    ))}
                  </div>
                </div>
              )}

              {analysis.detected_grammar.length > 0 && (
                <div className="flex flex-col gap-2">
                  <div className="text-xs font-bold" style={{ color: 'var(--text-muted)' }}>
                    Grammar Found in Text
                  </div>
                  {analysis.detected_grammar.map((dg) => (
                    <button
                      key={dg.grammar_id}
                      onClick={() => onSelectGrammar(dg.grammar_id)}
                      className="flex items-center justify-between rounded-xl border px-3 py-2 text-left text-xs cursor-pointer"
                      style={{
                        backgroundColor: 'var(--bg-canvas)',
                        borderColor: 'var(--border-subtle)',
                      }}
                    >
                      <span className="font-jp font-bold">
                        {dg.title} ({dg.jlpt_level})
                      </span>
                      <span style={{ color: 'var(--text-secondary)' }}>{dg.meaning}</span>
                    </button>
                  ))}
                </div>
              )}
            </div>
          )}
        </div>
      </div>
    </div>
  )
}

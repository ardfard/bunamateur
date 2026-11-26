import React, { useEffect, useMemo, useState } from 'react'
import {
  Bookmark,
  BookmarkPlus,
  BookMarked,
  BookOpen,
  Check,
  Download,
  FileJson,
  Search,
  Sparkles,
  Trash2,
  Volume2,
  Wand2,
  X,
} from 'lucide-react'
import { api } from '../api'
import { useToast } from '../components/ToastContext'
import type {
  AnalyzedToken,
  FuriganaMode,
  ReadingAnalyzeResponse,
  ReadingPassage,
  VocabBookmark,
} from '../types'
import { RubyText } from '../utils/furigana'
import { speakJapanese } from '../utils/speech'

interface ReadingLabPageProps {
  furiganaMode: FuriganaMode
  knownKanji: Set<string>
  highlightKnown: boolean
  onSelectGrammar: (id: number) => void
}

interface TokenStreamProps {
  tokens: AnalyzedToken[]
  furiganaMode: FuriganaMode
  knownKanji: Set<string>
  highlightKnown: boolean
  selectedToken: AnalyzedToken | null
  onSelectToken: (token: AnalyzedToken) => void
}

const InteractiveTokenStream: React.FC<TokenStreamProps> = ({
  tokens,
  furiganaMode,
  knownKanji,
  highlightKnown,
  selectedToken,
  onSelectToken,
}) => {
  return (
    <span className="font-jp leading-[2.3]">
      {tokens.map((tok, idx) => {
        const clickable = tok.is_content_word || tok.glosses.length > 0
        if (!clickable) {
          return (
            <span key={idx} className="inline">
              {tok.surface}
            </span>
          )
        }
        const isSelected =
          selectedToken?.surface === tok.surface && selectedToken?.lemma === tok.lemma
        return (
          <button
            key={idx}
            type="button"
            onClick={() => onSelectToken(tok)}
            className="mx-[1px] inline rounded-md px-0.5 transition-colors hover:underline cursor-pointer"
            style={{
              backgroundColor: isSelected
                ? 'var(--accent-indigo-soft)'
                : tok.wk_known && highlightKnown
                  ? 'rgba(236, 72, 153, 0.08)'
                  : 'transparent',
              outline: isSelected ? '1px solid var(--accent-indigo)' : 'none',
            }}
            title={
              tok.glosses.length > 0
                ? `${tok.reading} — ${tok.glosses[0]}`
                : tok.reading
            }
          >
            <RubyText
              text={tok.ruby}
              furiganaMode={furiganaMode}
              knownKanji={knownKanji}
              highlightKnown={highlightKnown}
            />
          </button>
        )
      })}
    </span>
  )
}

interface DictionaryInspectorProps {
  token: AnalyzedToken
  isSaved: boolean
  saving: boolean
  onSave: (token: AnalyzedToken) => void
  onClose: () => void
}

const DictionaryInspector: React.FC<DictionaryInspectorProps> = ({
  token,
  isSaved,
  saving,
  onSave,
  onClose,
}) => {
  const showLemma = token.lemma && token.lemma !== token.surface
  return (
    <div
      className="flex flex-col gap-2.5 rounded-xl border p-4 shadow-xs"
      style={{
        backgroundColor: 'var(--bg-canvas)',
        borderColor: 'var(--accent-indigo)',
      }}
    >
      <div className="flex flex-wrap items-start justify-between gap-2">
        <div className="flex flex-wrap items-center gap-2">
          <span className="font-jp text-xl font-bold">{token.surface}</span>
          {token.reading && token.reading !== token.surface && (
            <span className="font-jp text-sm" style={{ color: 'var(--text-secondary)' }}>
              【{token.reading}】
            </span>
          )}
          <span
            className="font-mono-code rounded-md px-2 py-0.5 text-[11px] font-semibold uppercase"
            style={{
              backgroundColor: 'var(--bg-subtle)',
              color: 'var(--text-secondary)',
            }}
          >
            {token.pos}
          </span>
          {token.jlpt && (
            <span
              className="font-mono-code rounded-md px-2 py-0.5 text-[11px] font-bold"
              style={{
                backgroundColor: 'var(--accent-indigo-soft)',
                color: 'var(--accent-indigo)',
              }}
            >
              {token.jlpt}
            </span>
          )}
          {token.wk_level !== null && (
            <span
              className="font-mono-code rounded-md px-2 py-0.5 text-[11px] font-bold"
              style={{
                backgroundColor: 'var(--wk-pink-soft)',
                color: 'var(--wk-pink)',
              }}
            >
              WK Lv.{token.wk_level}
              {token.wk_known ? ' • Known' : ''}
            </span>
          )}
          {token.wk_level === null && token.wk_known && (
            <span
              className="font-mono-code rounded-md px-2 py-0.5 text-[11px] font-bold"
              style={{
                backgroundColor: 'var(--accent-matcha-soft)',
                color: 'var(--accent-matcha)',
              }}
            >
              Kanji Known in WK
            </span>
          )}
        </div>

        <div className="flex items-center gap-1.5">
          {isSaved ? (
            <span
              className="inline-flex items-center gap-1 rounded-lg px-2.5 py-1 text-xs font-semibold"
              style={{
                backgroundColor: 'var(--accent-matcha-soft)',
                color: 'var(--accent-matcha)',
              }}
            >
              <Check className="h-3.5 w-3.5" />
              Saved ✓
            </span>
          ) : (
            <button
              type="button"
              onClick={() => onSave(token)}
              disabled={saving}
              className="inline-flex items-center gap-1.5 rounded-lg px-2.5 py-1 text-xs font-semibold text-white transition-opacity hover:opacity-90 disabled:opacity-60 cursor-pointer"
              style={{ backgroundColor: 'var(--accent-matcha)' }}
              title="Save word to your personal vocabulary deck"
            >
              <BookmarkPlus className="h-3.5 w-3.5" />
              {saving ? 'Saving...' : 'Save to Vocab Deck'}
            </button>
          )}
          <button
            type="button"
            onClick={() => speakJapanese(token.surface, 0.9)}
            className="rounded-lg p-1.5 hover:opacity-75 cursor-pointer"
            style={{ backgroundColor: 'var(--bg-surface)', color: 'var(--accent-indigo)' }}
            title="Pronounce word"
          >
            <Volume2 className="h-4 w-4" />
          </button>
          <button
            type="button"
            onClick={onClose}
            className="rounded-lg p-1.5 hover:opacity-75 cursor-pointer"
            style={{ backgroundColor: 'var(--bg-surface)', color: 'var(--text-muted)' }}
            title="Close dictionary card"
          >
            <X className="h-4 w-4" />
          </button>
        </div>
      </div>

      {showLemma && (
        <div className="text-xs" style={{ color: 'var(--text-secondary)' }}>
          Dictionary form (lemma):{' '}
          <span className="font-jp font-bold" style={{ color: 'var(--text-primary)' }}>
            {token.lemma}
          </span>
        </div>
      )}

      {token.glosses.length > 0 ? (
        <ol className="list-inside list-decimal space-y-0.5 text-sm">
          {token.glosses.map((g, idx) => (
            <li key={idx} style={{ color: 'var(--text-primary)' }}>
              {g}
            </li>
          ))}
        </ol>
      ) : (
        <div className="text-xs italic" style={{ color: 'var(--text-muted)' }}>
          No embedded JMdict gloss available for this token yet.
        </div>
      )}
    </div>
  )
}

export const ReadingLabPage: React.FC<ReadingLabPageProps> = ({
  furiganaMode,
  knownKanji,
  highlightKnown,
  onSelectGrammar,
}) => {
  const { toast } = useToast()
  const [passages, setPassages] = useState<ReadingPassage[]>([])
  const [selectedId, setSelectedId] = useState<number | null>(null)
  const [showEnglish, setShowEnglish] = useState(true)
  const [interactivePassage, setInteractivePassage] = useState(true)
  const [selectedPassageToken, setSelectedPassageToken] = useState<AnalyzedToken | null>(null)

  const [customText, setCustomText] = useState(
    '毎日日本語を勉強しているおかげで、少しずつ日本の新聞が読めるようになりました。雨にもかかわらず、公園へ行くつもりです。'
  )
  const [analysis, setAnalysis] = useState<ReadingAnalyzeResponse | null>(null)
  const [selectedCustomToken, setSelectedCustomToken] = useState<AnalyzedToken | null>(null)
  const [analyzing, setAnalyzing] = useState(false)

  const [bookmarks, setBookmarks] = useState<VocabBookmark[]>([])
  const [loadingBookmarks, setLoadingBookmarks] = useState(true)
  const [savingLemma, setSavingLemma] = useState<string | null>(null)
  const [deletingId, setDeletingId] = useState<number | null>(null)
  const [exportingFormat, setExportingFormat] = useState<'anki' | 'json' | null>(null)
  const [vocabSearch, setVocabSearch] = useState('')

  useEffect(() => {
    api.getReadingPassages().then((list) => {
      setPassages(list)
      if (list.length > 0) setSelectedId(list[0].id)
    })
    api
      .getVocabBookmarks()
      .then((list) => setBookmarks(list))
      .catch(() => {})
      .finally(() => setLoadingBookmarks(false))
  }, [])

  const activePassage = passages.find((p) => p.id === selectedId) || passages[0]

  const savedLemmas = useMemo(() => {
    const set = new Set<string>()
    for (const b of bookmarks) {
      if (b.lemma) set.add(b.lemma)
      else if (b.surface) set.add(b.surface)
    }
    return set
  }, [bookmarks])

  const filteredBookmarks = useMemo(() => {
    const q = vocabSearch.trim().toLowerCase()
    if (!q) return bookmarks
    return bookmarks.filter((b) => {
      if (b.surface.toLowerCase().includes(q)) return true
      if (b.lemma.toLowerCase().includes(q)) return true
      if (b.reading.toLowerCase().includes(q)) return true
      if (b.glosses.some((g) => g.toLowerCase().includes(q))) return true
      return false
    })
  }, [bookmarks, vocabSearch])

  const isTokenSaved = (token: AnalyzedToken) =>
    savedLemmas.has(token.lemma || token.surface)

  const handleSaveToken = async (token: AnalyzedToken, contextSentence: string) => {
    const lemmaKey = token.lemma || token.surface
    if (!lemmaKey) return
    setSavingLemma(lemmaKey)
    try {
      const saved = await api.saveVocabBookmark({
        surface: token.surface,
        lemma: lemmaKey,
        reading: token.reading,
        pos: token.pos,
        glosses: token.glosses,
        jlpt: token.jlpt,
        wk_level: token.wk_level,
        context_sentence: contextSentence,
      })
      setBookmarks((prev) => {
        const filtered = prev.filter((b) => b.id !== saved.id && b.lemma !== saved.lemma)
        return [saved, ...filtered]
      })
      toast.success(`Saved "${saved.surface}" to your Vocab Deck.`)
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Failed to save vocabulary word.')
    } finally {
      setSavingLemma(null)
    }
  }

  const handleDeleteBookmark = async (bookmark: VocabBookmark) => {
    setDeletingId(bookmark.id)
    try {
      await api.deleteVocabBookmark(bookmark.id)
      setBookmarks((prev) => prev.filter((b) => b.id !== bookmark.id))
      toast.info(`Removed "${bookmark.surface}" from your Vocab Deck.`)
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Failed to remove vocabulary word.')
    } finally {
      setDeletingId(null)
    }
  }

  const handleExportVocab = async (format: 'anki' | 'json') => {
    setExportingFormat(format)
    try {
      const content = await api.exportVocabBookmarksText(format)
      const blob = new Blob([content], {
        type:
          format === 'anki'
            ? 'text/tab-separated-values;charset=utf-8'
            : 'application/json;charset=utf-8',
      })
      const filename =
        format === 'anki' ? 'bunamateur-vocab-anki.tsv' : 'bunamateur-vocab.json'
      const url = URL.createObjectURL(blob)
      const a = document.createElement('a')
      a.href = url
      a.download = filename
      a.click()
      URL.revokeObjectURL(url)
      toast.success(
        format === 'anki'
          ? `Exported ${bookmarks.length} words to Anki TSV (${filename}).`
          : `Exported ${bookmarks.length} words as JSON (${filename}).`
      )
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Failed to export vocabulary deck.')
    } finally {
      setExportingFormat(null)
    }
  }

  const customContextSnippet = useMemo(() => {
    const cleaned = customText.trim().replace(/\s+/g, ' ')
    if (cleaned.length <= 120) return cleaned
    return `${cleaned.slice(0, 120)}…`
  }, [customText])

  const handleAnalyze = async () => {
    if (!customText.trim()) return
    setAnalyzing(true)
    setSelectedCustomToken(null)
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
          Click any word to inspect JMdict definitions, dictionary lemmas &amp; WaniKani levels, or paste Japanese text for automatic furigana &amp; lemmatized JLPT grammar detection.
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
                  onClick={() => {
                    setSelectedId(p.id)
                    setSelectedPassageToken(null)
                  }}
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

                <div className="flex flex-wrap items-center gap-2">
                  <button
                    onClick={() => setInteractivePassage((v) => !v)}
                    className="flex items-center gap-1.5 rounded-lg border px-3 py-1.5 text-xs font-semibold cursor-pointer"
                    style={{
                      backgroundColor: interactivePassage
                        ? 'var(--accent-indigo-soft)'
                        : 'var(--bg-subtle)',
                      borderColor: interactivePassage
                        ? 'var(--accent-indigo)'
                        : 'var(--border-subtle)',
                      color: interactivePassage
                        ? 'var(--accent-indigo)'
                        : 'var(--text-secondary)',
                    }}
                    title="Toggle click-to-inspect JMdict word lookup"
                  >
                    <BookOpen className="h-3.5 w-3.5" />
                    {interactivePassage ? 'JMdict Popover On' : 'Plain Ruby'}
                  </button>
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
                {interactivePassage && activePassage.tokens && activePassage.tokens.length > 0 ? (
                  <InteractiveTokenStream
                    tokens={activePassage.tokens}
                    furiganaMode={furiganaMode}
                    knownKanji={knownKanji}
                    highlightKnown={highlightKnown}
                    selectedToken={selectedPassageToken}
                    onSelectToken={setSelectedPassageToken}
                  />
                ) : (
                  <RubyText
                    text={activePassage.content_ruby}
                    furiganaMode={furiganaMode}
                    knownKanji={knownKanji}
                    highlightKnown={highlightKnown}
                  />
                )}
              </div>

              {selectedPassageToken && (
                <DictionaryInspector
                  token={selectedPassageToken}
                  isSaved={isTokenSaved(selectedPassageToken)}
                  saving={
                    savingLemma === (selectedPassageToken.lemma || selectedPassageToken.surface)
                  }
                  onSave={(tok) => handleSaveToken(tok, activePassage.title)}
                  onClose={() => setSelectedPassageToken(null)}
                />
              )}

              {showEnglish && (
                <p className="text-sm leading-relaxed" style={{ color: 'var(--text-secondary)' }}>
                  {activePassage.english}
                </p>
              )}

              {/* Detected Grammar Chips */}
              <div className="flex flex-col gap-2.5">
                <div
                  className="text-xs font-bold uppercase tracking-wider"
                  style={{ color: 'var(--text-muted)' }}
                >
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
                          <span
                            className="font-mono-code text-[11px] font-semibold"
                            style={{ color: 'var(--accent-indigo)' }}
                          >
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
                          color:
                            dg.srs_stage > 0 ? 'var(--accent-matcha)' : 'var(--accent-vermilion)',
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
            Paste a news paragraph, manga dialogue, or diary entry. Pure-Rust morphological NLP auto-generates furigana, lemmatizes verbs/adjectives for JLPT grammar detection, and provides click-to-inspect JMdict lookups.
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
            {analyzing ? 'Tokenizing & Analyzing...' : 'Analyze Auto-Furigana, Grammar & JMdict'}
          </button>

          {analysis && (
            <div className="flex flex-col gap-4 pt-2">
              <div className="grid grid-cols-3 gap-2.5">
                <div
                  className="rounded-xl p-3"
                  style={{ backgroundColor: 'var(--accent-matcha-soft)' }}
                >
                  <div
                    className="font-mono-code text-lg font-bold"
                    style={{ color: 'var(--accent-matcha)' }}
                  >
                    {analysis.wk_readiness_pct}%
                  </div>
                  <div className="text-[11px]" style={{ color: 'var(--text-secondary)' }}>
                    WK Readable
                  </div>
                </div>
                <div className="rounded-xl p-3" style={{ backgroundColor: 'var(--wk-pink-soft)' }}>
                  <div
                    className="font-mono-code text-lg font-bold"
                    style={{ color: 'var(--wk-pink)' }}
                  >
                    {analysis.known_kanji}/{analysis.total_kanji}
                  </div>
                  <div className="text-[11px]" style={{ color: 'var(--text-secondary)' }}>
                    Kanji Known
                  </div>
                </div>
                <div
                  className="rounded-xl p-3"
                  style={{ backgroundColor: 'var(--accent-indigo-soft)' }}
                >
                  <div
                    className="font-mono-code text-lg font-bold"
                    style={{ color: 'var(--accent-indigo)' }}
                  >
                    {analysis.detected_grammar.length}
                  </div>
                  <div className="text-[11px]" style={{ color: 'var(--text-secondary)' }}>
                    Grammar Hits
                  </div>
                </div>
              </div>

              {/* Interactive Auto-Furigana + JMdict Tokenized View */}
              {analysis.tokens.length > 0 && (
                <div className="flex flex-col gap-2">
                  <div className="flex items-center justify-between">
                    <div
                      className="text-xs font-bold uppercase tracking-wider"
                      style={{ color: 'var(--text-muted)' }}
                    >
                      Auto-Furigana &amp; Interactive Tokens (Click Any Word)
                    </div>
                    <span
                      className="font-mono-code text-[10px]"
                      style={{ color: 'var(--text-muted)' }}
                    >
                      {analysis.tokens.length} tokens
                    </span>
                  </div>
                  <div
                    className="rounded-xl border p-4 text-base"
                    style={{
                      backgroundColor: 'var(--bg-canvas)',
                      borderColor: 'var(--border-subtle)',
                    }}
                  >
                    <InteractiveTokenStream
                      tokens={analysis.tokens}
                      furiganaMode={furiganaMode}
                      knownKanji={knownKanji}
                      highlightKnown={highlightKnown}
                      selectedToken={selectedCustomToken}
                      onSelectToken={setSelectedCustomToken}
                    />
                  </div>
                </div>
              )}

              {selectedCustomToken && (
                <DictionaryInspector
                  token={selectedCustomToken}
                  isSaved={isTokenSaved(selectedCustomToken)}
                  saving={
                    savingLemma === (selectedCustomToken.lemma || selectedCustomToken.surface)
                  }
                  onSave={(tok) => handleSaveToken(tok, customContextSnippet)}
                  onClose={() => setSelectedCustomToken(null)}
                />
              )}

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
                    Grammar Found in Text (Lemmatized)
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

      {/* Personal Saved Vocabulary Deck & 1-Click Anki Export */}
      <section
        className="flex flex-col gap-5 rounded-2xl border p-6"
        style={{
          backgroundColor: 'var(--bg-surface)',
          borderColor: 'var(--border-subtle)',
        }}
      >
        <div className="flex flex-col gap-4 md:flex-row md:items-center md:justify-between">
          <div className="flex flex-col gap-1">
            <div className="flex flex-wrap items-center gap-2.5">
              <Bookmark className="h-5 w-5" style={{ color: 'var(--accent-indigo)' }} />
              <h2 className="text-lg font-bold tracking-tight">My Saved Vocab Deck</h2>
              <span
                className="font-mono-code rounded-full px-2.5 py-0.5 text-xs font-bold"
                style={{
                  backgroundColor: 'var(--accent-indigo-soft)',
                  color: 'var(--accent-indigo)',
                }}
              >
                {bookmarks.length} {bookmarks.length === 1 ? 'word' : 'words'}
              </span>
            </div>
            <p className="text-xs" style={{ color: 'var(--text-secondary)' }}>
              Words bookmarked from curated passages and custom text analysis — ready for 1-click Anki import.
            </p>
          </div>

          <div className="flex flex-wrap items-center gap-2">
            <div className="relative min-w-[210px] flex-1 sm:flex-initial">
              <Search
                className="pointer-events-none absolute top-1/2 left-3 h-3.5 w-3.5 -translate-y-1/2"
                style={{ color: 'var(--text-muted)' }}
              />
              <input
                type="text"
                value={vocabSearch}
                onChange={(e) => setVocabSearch(e.target.value)}
                placeholder="Filter surface, reading, gloss..."
                className="w-full rounded-xl border py-1.5 pr-8 pl-8 text-xs outline-none"
                style={{
                  backgroundColor: 'var(--bg-canvas)',
                  borderColor: 'var(--border-subtle)',
                  color: 'var(--text-primary)',
                }}
              />
              {vocabSearch && (
                <button
                  type="button"
                  onClick={() => setVocabSearch('')}
                  className="absolute top-1/2 right-2 -translate-y-1/2 rounded p-0.5 hover:opacity-75 cursor-pointer"
                  style={{ color: 'var(--text-muted)' }}
                  aria-label="Clear vocabulary search"
                >
                  <X className="h-3.5 w-3.5" />
                </button>
              )}
            </div>

            <button
              type="button"
              onClick={() => handleExportVocab('anki')}
              disabled={bookmarks.length === 0 || exportingFormat !== null}
              className="inline-flex items-center gap-1.5 rounded-xl px-3.5 py-1.5 text-xs font-semibold text-white transition-opacity hover:opacity-90 disabled:opacity-50 cursor-pointer"
              style={{ backgroundColor: 'var(--accent-indigo)' }}
            >
              <Download className="h-3.5 w-3.5" />
              {exportingFormat === 'anki' ? 'Exporting...' : 'Export Anki (.tsv)'}
            </button>

            <button
              type="button"
              onClick={() => handleExportVocab('json')}
              disabled={bookmarks.length === 0 || exportingFormat !== null}
              className="inline-flex items-center gap-1.5 rounded-xl border px-3.5 py-1.5 text-xs font-semibold transition-opacity hover:opacity-85 disabled:opacity-50 cursor-pointer"
              style={{
                backgroundColor: 'var(--bg-subtle)',
                borderColor: 'var(--border-subtle)',
                color: 'var(--text-primary)',
              }}
            >
              <FileJson className="h-3.5 w-3.5" />
              {exportingFormat === 'json' ? 'Exporting...' : 'Export JSON'}
            </button>
          </div>
        </div>

        {loadingBookmarks ? (
          <div
            className="rounded-xl border border-dashed px-6 py-10 text-center text-sm"
            style={{
              backgroundColor: 'var(--bg-canvas)',
              borderColor: 'var(--border-subtle)',
              color: 'var(--text-muted)',
            }}
          >
            Loading your saved vocabulary deck...
          </div>
        ) : bookmarks.length === 0 ? (
          <div
            className="flex flex-col items-center justify-center gap-2 rounded-xl border border-dashed px-6 py-10 text-center"
            style={{
              backgroundColor: 'var(--bg-canvas)',
              borderColor: 'var(--border-subtle)',
            }}
          >
            <BookmarkPlus className="h-7 w-7" style={{ color: 'var(--text-muted)' }} />
            <p className="max-w-lg text-sm" style={{ color: 'var(--text-secondary)' }}>
              Click any Japanese word in a reading passage or analyzed text above and hit &apos;Save to Vocab Deck&apos; to build your personal Anki study deck.
            </p>
          </div>
        ) : filteredBookmarks.length === 0 ? (
          <div
            className="flex flex-col items-center justify-center gap-2 rounded-xl border border-dashed px-6 py-8 text-center"
            style={{
              backgroundColor: 'var(--bg-canvas)',
              borderColor: 'var(--border-subtle)',
            }}
          >
            <p className="text-sm" style={{ color: 'var(--text-secondary)' }}>
              No saved words match &ldquo;{vocabSearch}&rdquo;.
            </p>
            <button
              type="button"
              onClick={() => setVocabSearch('')}
              className="rounded-lg border px-3 py-1 text-xs font-semibold hover:opacity-80 cursor-pointer"
              style={{
                borderColor: 'var(--border-subtle)',
                color: 'var(--accent-indigo)',
              }}
            >
              Clear filter
            </button>
          </div>
        ) : (
          <div className="grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-3">
            {filteredBookmarks.map((b) => {
              const showLemma = b.lemma && b.lemma !== b.surface
              const showReading = b.reading && b.reading !== b.surface
              return (
                <div
                  key={b.id}
                  className="flex flex-col justify-between gap-2.5 rounded-xl border p-4 transition-colors"
                  style={{
                    backgroundColor: 'var(--bg-canvas)',
                    borderColor: 'var(--border-subtle)',
                  }}
                >
                  <div className="flex flex-col gap-2">
                    <div className="flex items-start justify-between gap-2">
                      <div className="flex flex-wrap items-baseline gap-x-2 gap-y-0.5">
                        <span className="font-jp text-lg font-bold">{b.surface}</span>
                        {showReading && (
                          <span
                            className="font-jp text-xs"
                            style={{ color: 'var(--text-secondary)' }}
                          >
                            【{b.reading}】
                          </span>
                        )}
                        {showLemma && (
                          <span
                            className="font-jp text-xs"
                            style={{ color: 'var(--text-muted)' }}
                          >
                            ({b.lemma})
                          </span>
                        )}
                      </div>

                      <div className="flex shrink-0 items-center gap-1">
                        <button
                          type="button"
                          onClick={() => speakJapanese(b.surface, 0.9)}
                          className="rounded-lg p-1.5 transition-opacity hover:opacity-75 cursor-pointer"
                          style={{
                            backgroundColor: 'var(--bg-surface)',
                            color: 'var(--accent-indigo)',
                          }}
                          title={`Pronounce ${b.surface}`}
                          aria-label={`Pronounce ${b.surface}`}
                        >
                          <Volume2 className="h-3.5 w-3.5" />
                        </button>
                        <button
                          type="button"
                          onClick={() => handleDeleteBookmark(b)}
                          disabled={deletingId === b.id}
                          className="rounded-lg p-1.5 transition-opacity hover:opacity-75 disabled:opacity-50 cursor-pointer"
                          style={{
                            backgroundColor: 'var(--bg-surface)',
                            color: 'var(--accent-vermilion)',
                          }}
                          title={`Remove ${b.surface} from Vocab Deck`}
                          aria-label={`Remove ${b.surface} from Vocab Deck`}
                        >
                          <Trash2 className="h-3.5 w-3.5" />
                        </button>
                      </div>
                    </div>

                    <div className="flex flex-wrap items-center gap-1.5">
                      {b.pos && (
                        <span
                          className="font-mono-code rounded-md px-1.5 py-0.5 text-[10px] font-semibold uppercase"
                          style={{
                            backgroundColor: 'var(--bg-subtle)',
                            color: 'var(--text-secondary)',
                          }}
                        >
                          {b.pos}
                        </span>
                      )}
                      {b.jlpt && (
                        <span
                          className="font-mono-code rounded-md px-1.5 py-0.5 text-[10px] font-bold"
                          style={{
                            backgroundColor: 'var(--accent-indigo-soft)',
                            color: 'var(--accent-indigo)',
                          }}
                        >
                          {b.jlpt}
                        </span>
                      )}
                      {b.wk_level !== null && (
                        <span
                          className="font-mono-code rounded-md px-1.5 py-0.5 text-[10px] font-bold"
                          style={{
                            backgroundColor: 'var(--wk-pink-soft)',
                            color: 'var(--wk-pink)',
                          }}
                        >
                          WK Lv.{b.wk_level}
                        </span>
                      )}
                    </div>

                    {b.glosses.length > 0 ? (
                      <p
                        className="text-xs leading-relaxed"
                        style={{ color: 'var(--text-primary)' }}
                      >
                        {b.glosses.join('; ')}
                      </p>
                    ) : (
                      <p className="text-xs italic" style={{ color: 'var(--text-muted)' }}>
                        No English gloss recorded.
                      </p>
                    )}
                  </div>

                  {b.context_sentence && (
                    <div
                      className="font-jp truncate rounded-lg px-2.5 py-1.5 text-[11px]"
                      style={{
                        backgroundColor: 'var(--bg-subtle)',
                        color: 'var(--text-secondary)',
                      }}
                      title={b.context_sentence}
                    >
                      {b.context_sentence}
                    </div>
                  )}
                </div>
              )
            })}
          </div>
        )}
      </section>
    </div>
  )
}

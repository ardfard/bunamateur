import React, { useEffect, useState } from 'react'
import { Ghost, Search, AlertCircle } from 'lucide-react'
import { api } from '../api'
import type { GrammarPointSummary } from '../types'

interface GrammarPageProps {
  onSelectGrammar: (id: number, slug?: string) => void
  refreshKey: number
}

export const GrammarPage: React.FC<GrammarPageProps> = ({
  onSelectGrammar,
  refreshKey,
}) => {
  const [items, setItems] = useState<GrammarPointSummary[]>([])
  const [jlpt, setJlpt] = useState('ALL')
  const [status, setStatus] = useState('all')
  const [query, setQuery] = useState('')
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    setLoading(true)
    const timer = setTimeout(() => {
      api
        .getGrammarList({ jlpt, status, q: query })
        .then(setItems)
        .finally(() => setLoading(false))
    }, 120)
    return () => clearTimeout(timer)
  }, [jlpt, status, query, refreshKey])

  return (
    <div className="mx-auto flex max-w-[1360px] flex-col gap-6 px-4 py-8 sm:px-8">
      {/* Header + JLPT Pills */}
      <div className="flex flex-col justify-between gap-4 lg:flex-row lg:items-end">
        <div>
          <h1 className="text-2xl font-bold tracking-tight">Grammar Library</h1>
          <p className="mt-1 text-sm" style={{ color: 'var(--text-secondary)' }}>
            Browse curated JLPT N5–N1 grammar points with WaniKani Kanji Readiness and nuance contrasts.
          </p>
        </div>

        <div className="flex flex-wrap items-center gap-1.5">
          {['ALL', 'N5', 'N4', 'N3', 'N2', 'N1'].map((lvl) => {
            const active = jlpt === lvl
            return (
              <button
                key={lvl}
                onClick={() => setJlpt(lvl)}
                className="font-mono-code rounded-full border px-3.5 py-1.5 text-xs font-semibold transition-colors cursor-pointer"
                style={{
                  backgroundColor: active ? 'var(--accent-vermilion)' : 'var(--bg-surface)',
                  borderColor: active ? 'var(--accent-vermilion)' : 'var(--border-subtle)',
                  color: active ? '#ffffff' : 'var(--text-secondary)',
                }}
              >
                {lvl === 'ALL' ? 'All Levels' : lvl}
              </button>
            )
          })}
        </div>
      </div>

      {/* Search & Status Filters */}
      <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
        <div
          className="flex flex-1 max-w-md items-center gap-2.5 rounded-xl border px-3.5 py-2.5"
          style={{
            backgroundColor: 'var(--bg-surface)',
            borderColor: 'var(--border-subtle)',
          }}
        >
          <Search className="h-4 w-4 shrink-0" style={{ color: 'var(--text-muted)' }} />
          <input
            type="text"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search Japanese, meaning, or textbook (e.g. Genki, のに)..."
            className="w-full bg-transparent text-sm outline-none"
            style={{ color: 'var(--text-primary)' }}
          />
        </div>

        <div className="flex flex-wrap items-center gap-1.5">
          {(
            [
              ['all', 'All'],
              ['learned', 'Studied'],
              ['new', 'New Lessons'],
              ['ghost', 'Ghosts'],
              ['leech', 'Leeches'],
            ] as [string, string][]
          ).map(([st, lbl]) => (
            <button
              key={st}
              onClick={() => setStatus(st)}
              className="rounded-lg border px-3 py-1.5 text-xs font-medium transition-colors cursor-pointer"
              style={{
                backgroundColor: status === st ? 'var(--bg-subtle)' : 'var(--bg-surface)',
                borderColor: status === st ? 'var(--text-primary)' : 'var(--border-subtle)',
                color: status === st ? 'var(--text-primary)' : 'var(--text-secondary)',
              }}
            >
              {lbl}
            </button>
          ))}
        </div>
      </div>

      {/* Cards Grid */}
      {loading ? (
        <div className="py-16 text-center text-sm" style={{ color: 'var(--text-muted)' }}>
          Loading grammar points...
        </div>
      ) : items.length === 0 ? (
        <div
          className="rounded-2xl border py-16 text-center text-sm"
          style={{
            backgroundColor: 'var(--bg-surface)',
            borderColor: 'var(--border-subtle)',
            color: 'var(--text-secondary)',
          }}
        >
          No grammar points match your current filters.
        </div>
      ) : (
        <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3">
          {items.map((g) => (
            <button
              key={g.id}
              onClick={() => onSelectGrammar(g.id, g.slug)}
              className="group flex flex-col justify-between gap-3 rounded-2xl border p-5 text-left transition-all hover:-translate-y-0.5 cursor-pointer"
              style={{
                backgroundColor: 'var(--bg-surface)',
                borderColor: 'var(--border-subtle)',
              }}
            >
              <div className="flex flex-col gap-2">
                <div className="flex items-start justify-between gap-2">
                  <span className="font-jp text-xl font-bold group-hover:underline">
                    {g.title}
                  </span>
                  <div className="flex items-center gap-1.5">
                    {g.ghost_level > 0 && (
                      <span
                        className="flex items-center gap-0.5 rounded-md px-1.5 py-0.5 text-[10px] font-bold"
                        style={{
                          backgroundColor: 'var(--accent-vermilion-soft)',
                          color: 'var(--accent-vermilion)',
                        }}
                        title={`Ghost Review Level ${g.ghost_level}`}
                      >
                        <Ghost className="h-3 w-3" />
                        {g.ghost_level}
                      </span>
                    )}
                    {g.is_leech && (
                      <span
                        className="flex items-center gap-0.5 rounded-md px-1.5 py-0.5 text-[10px] font-bold"
                        style={{
                          backgroundColor: 'var(--accent-amber-soft)',
                          color: 'var(--accent-amber)',
                        }}
                        title="Leech: frequently missed"
                      >
                        <AlertCircle className="h-3 w-3" />
                      </span>
                    )}
                    <span
                      className="font-mono-code rounded-md px-2 py-0.5 text-[11px] font-bold"
                      style={{
                        backgroundColor: 'var(--accent-indigo-soft)',
                        color: 'var(--accent-indigo)',
                      }}
                    >
                      {g.jlpt_level}
                    </span>
                  </div>
                </div>

                <p className="text-sm leading-snug" style={{ color: 'var(--text-secondary)' }}>
                  {g.meaning}
                </p>

                <div
                  className="font-mono-code truncate text-[11px]"
                  style={{ color: 'var(--text-muted)' }}
                >
                  {g.structure}
                </div>
              </div>

              <div
                className="flex items-center justify-between border-t pt-3 font-mono-code text-[11px]"
                style={{ borderColor: 'var(--border-subtle)' }}
              >
                <span
                  style={{
                    color: g.srs_stage > 0 ? 'var(--accent-matcha)' : 'var(--text-muted)',
                    fontWeight: 600,
                  }}
                >
                  {g.srs_label}
                </span>
                <span style={{ color: 'var(--wk-pink)', fontWeight: 600 }}>
                  {g.wk_total_kanji > 0 ? `${g.wk_readiness_pct}% WK Kanji` : g.study_path}
                </span>
              </div>
            </button>
          ))}
        </div>
      )}
    </div>
  )
}

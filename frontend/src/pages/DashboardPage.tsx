import React, { useRef, useState } from 'react'
import {
  BookOpen,
  Download,
  Ghost,
  Palmtree,
  Play,
  RefreshCw,
  Sparkles,
  Swords,
  BookMarked,
  Upload,
  Zap,
} from 'lucide-react'
import { api } from '../api'
import { OnboardingPanel } from '../components/OnboardingPanel'
import { StudyAnalyticsPanel } from '../components/StudyAnalyticsPanel'
import type { DashboardStats, PageTab, ReviewMode } from '../types'

interface DashboardPageProps {
  stats: DashboardStats
  onStartSession: (mode: ReviewMode) => void
  onNavigate: (tab: PageTab) => void
  onRefresh: () => void
  onSelectGrammar?: (id: number) => void
}

const JLPT_COLORS: Record<string, string> = {
  N5: 'var(--accent-matcha)',
  N4: 'var(--accent-indigo)',
  N3: 'var(--accent-amber)',
  N2: 'var(--accent-vermilion)',
  N1: 'var(--wk-purple)',
}

const SRS_DOT_COLORS: Record<string, string> = {
  Apprentice: 'var(--wk-pink)',
  Guru: 'var(--wk-purple)',
  Master: 'var(--accent-indigo)',
  Enlightened: 'var(--accent-matcha)',
  Burned: 'var(--accent-amber)',
}

export const DashboardPage: React.FC<DashboardPageProps> = ({
  stats,
  onStartSession,
  onNavigate,
  onRefresh,
  onSelectGrammar,
}) => {
  const [syncingDemo, setSyncingDemo] = useState(false)
  const [updatingSrs, setUpdatingSrs] = useState(false)
  const [dataMsg, setDataMsg] = useState<string | null>(null)
  const importInputRef = useRef<HTMLInputElement>(null)

  const maxForecast = Math.max(1, ...stats.forecast_24h.map((b) => b.count))
  const totalForecast = stats.forecast_24h.reduce((sum, b) => sum + b.count, 0)
  const goalPct = Math.min(100, Math.round((stats.today_reviews / Math.max(1, stats.daily_goal)) * 100))

  const handleQuickDemoWK = async () => {
    setSyncingDemo(true)
    try {
      await api.demoWaniKani()
      onRefresh()
    } finally {
      setSyncingDemo(false)
    }
  }

  const handleToggleVacation = async () => {
    setUpdatingSrs(true)
    try {
      await api.updateSrsSettings({ vacation_mode: !stats.srs_settings.vacation_mode })
      onRefresh()
    } finally {
      setUpdatingSrs(false)
    }
  }

  const handleGhostIntensity = async (intensity: string) => {
    setUpdatingSrs(true)
    try {
      await api.updateSrsSettings({ ghost_intensity: intensity })
      onRefresh()
    } finally {
      setUpdatingSrs(false)
    }
  }

  const handleStudyPathChange = async (path: string) => {
    setUpdatingSrs(true)
    try {
      await api.updateSrsSettings({ preferred_study_path: path })
      onRefresh()
    } finally {
      setUpdatingSrs(false)
    }
  }

  const handleExport = async (format: 'json' | 'csv') => {
    setDataMsg(null)
    try {
      const content = await api.exportProgressText(format)
      const blob = new Blob([content], {
        type: format === 'csv' ? 'text/csv;charset=utf-8' : 'application/json;charset=utf-8',
      })
      const url = URL.createObjectURL(blob)
      const a = document.createElement('a')
      a.href = url
      a.download = `bunamateur-progress.${format}`
      a.click()
      URL.revokeObjectURL(url)
      setDataMsg(`Exported progress as ${format.toUpperCase()}.`)
    } catch (err) {
      setDataMsg(err instanceof Error ? err.message : 'Export failed')
    }
  }

  const handleImportFile = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0]
    if (!file) return
    setDataMsg(null)
    try {
      const text = await file.text()
      const fmt: 'json' | 'csv' = file.name.toLowerCase().endsWith('.csv') ? 'csv' : 'json'
      const res = await api.importProgress(fmt, text)
      setDataMsg(`Imported ${res.imported} grammar items (${res.skipped} skipped).`)
      onRefresh()
    } catch (err) {
      setDataMsg(err instanceof Error ? err.message : 'Import failed')
    } finally {
      e.target.value = ''
    }
  }

  return (
    <div className="mx-auto flex max-w-[1360px] flex-col gap-6 px-4 py-8 sm:px-8">
      {/* Zero-State Onboarding Panel for new accounts */}
      {!stats.onboarding.completed && <OnboardingPanel onCompleted={onRefresh} />}

      {/* Vacation Mode Banner */}
      {stats.srs_settings.vacation_mode && (
        <div
          className="flex flex-wrap items-center justify-between gap-3 rounded-2xl border px-5 py-3.5 text-sm"
          style={{
            backgroundColor: 'var(--accent-amber-soft)',
            borderColor: 'var(--accent-amber)',
            color: 'var(--accent-amber)',
          }}
        >
          <div className="flex items-center gap-2 font-semibold">
            <Palmtree className="h-4 w-4 shrink-0" />
            <span>
              Vacation Mode is active — due schedules are paused and your {stats.streak_days}-day streak is protected.
            </span>
          </div>
          <button
            type="button"
            disabled={updatingSrs}
            onClick={handleToggleVacation}
            className="rounded-lg px-3 py-1.5 text-xs font-bold text-white cursor-pointer"
            style={{ backgroundColor: 'var(--accent-amber)' }}
          >
            Resume Reviews
          </button>
        </div>
      )}
      {/* Hero Study Banner */}
      <section
        className="flex flex-col justify-between gap-6 rounded-2xl border p-6 sm:p-7 lg:flex-row lg:items-center"
        style={{
          backgroundColor: 'var(--bg-surface)',
          borderColor: 'var(--border-subtle)',
        }}
      >
        <div className="flex flex-col gap-1.5">
          <h1 className="font-jp text-2xl font-bold tracking-tight">
            おかえりなさい — Ready for today&apos;s study?
          </h1>
          <p className="text-sm leading-relaxed" style={{ color: 'var(--text-secondary)' }}>
            {stats.reviews_due > 0 ? (
              <>
                <strong>{stats.reviews_due}</strong> grammar reviews due now
                {stats.ghost_reviews_due > 0 && (
                  <>
                    , including{' '}
                    <span style={{ color: 'var(--accent-vermilion)', fontWeight: 600 }}>
                      {stats.ghost_reviews_due} Ghost Reviews
                    </span>
                  </>
                )}
                .
              </>
            ) : (
              'All caught up on reviews! Start a new lesson or drill in Cram Mode.'
            )}{' '}
            {stats.wanikani.connected
              ? `WaniKani Smart Furigana is auto-hiding ${stats.wanikani.kanji_known} mastered kanji.`
              : 'Connect WaniKani to auto-hide furigana on kanji you already know.'}
          </p>
        </div>

        <div className="flex flex-wrap items-center gap-3">
          <button
            onClick={() => onStartSession('review')}
            className="flex items-center gap-2 rounded-xl px-5 py-3 text-sm font-semibold text-white shadow-xs transition-transform active:scale-[0.98] cursor-pointer"
            style={{ backgroundColor: 'var(--accent-vermilion)' }}
          >
            <Play className="h-4 w-4 fill-current" />
            <span>Start Reviews ({stats.reviews_due})</span>
          </button>

          <button
            onClick={() => onStartSession('lesson')}
            className="flex items-center gap-2 rounded-xl px-5 py-3 text-sm font-semibold text-white shadow-xs transition-transform active:scale-[0.98] cursor-pointer"
            style={{ backgroundColor: 'var(--accent-indigo)' }}
          >
            <BookOpen className="h-4 w-4" />
            <span>Learn New ({stats.lessons_available})</span>
          </button>

          {stats.ghost_reviews_due > 0 && (
            <button
              onClick={() => onStartSession('ghost')}
              className="flex items-center gap-2 rounded-xl border px-4 py-3 text-sm font-semibold transition-colors cursor-pointer"
              style={{
                backgroundColor: 'var(--accent-vermilion-soft)',
                borderColor: 'var(--accent-vermilion)',
                color: 'var(--accent-vermilion)',
              }}
            >
              <Ghost className="h-4 w-4" />
              <span>Ghosts ({stats.ghost_reviews_due})</span>
            </button>
          )}

          <button
            onClick={() => onStartSession('cram')}
            className="flex items-center gap-2 rounded-xl border px-4 py-3 text-sm font-semibold transition-colors cursor-pointer"
            style={{
              backgroundColor: 'var(--bg-subtle)',
              borderColor: 'var(--border-subtle)',
              color: 'var(--text-primary)',
            }}
          >
            <Zap className="h-4 w-4" />
            <span>Cram Mode</span>
          </button>
        </div>
      </section>

      {/* 4 KPI Cards Row */}
      <section className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-4">
        <MetricCard
          label="Reviews Due"
          badge={stats.ghost_reviews_due > 0 ? `${stats.ghost_reviews_due} Ghosts` : 'Clear'}
          badgeColor={stats.ghost_reviews_due > 0 ? 'var(--accent-vermilion)' : 'var(--accent-matcha)'}
          value={String(stats.reviews_due)}
          sub={`Today: ${stats.today_reviews} / ${stats.daily_goal} daily goal (${goalPct}%)`}
        />
        <MetricCard
          label="Grammar Learned"
          badge={`${Math.round((stats.total_learned / Math.max(1, stats.total_grammar)) * 100)}%`}
          badgeColor="var(--accent-indigo)"
          value={`${stats.total_learned} / ${stats.total_grammar}`}
          sub="Across JLPT N5 – N1 study decks"
        />
        <MetricCard
          label="Accuracy (7d)"
          badge={`${stats.reviews_7d} reviews`}
          badgeColor="var(--accent-matcha)"
          value={`${stats.accuracy_7d}%`}
          sub={
            stats.leech_count > 0
              ? `${stats.leech_count} leech item(s) flagged for extra drill`
              : 'Zero leeches — solid retention!'
          }
        />
        <MetricCard
          label="WaniKani Kanji"
          badge={stats.wanikani.connected ? `Lv. ${stats.wanikani.level}` : 'Not linked'}
          badgeColor="var(--wk-pink)"
          value={stats.wanikani.connected ? String(stats.wanikani.kanji_known) : '—'}
          sub={
            stats.wanikani.connected
              ? 'Auto-hiding mastered furigana'
              : 'Link token or try Demo Lv.14'
          }
        />
      </section>

      {/* Main Two-Column Content */}
      <section className="grid grid-cols-1 gap-6 lg:grid-cols-12">
        {/* Left Column (7/12): JLPT Mastery + 24h Forecast */}
        <div className="flex flex-col gap-6 lg:col-span-7">
          {/* JLPT Grammar Mastery */}
          <div
            className="flex flex-col gap-4 rounded-2xl border p-6"
            style={{
              backgroundColor: 'var(--bg-surface)',
              borderColor: 'var(--border-subtle)',
            }}
          >
            <div className="flex items-center justify-between">
              <h2 className="text-base font-bold">JLPT Grammar Mastery</h2>
              <button
                onClick={() => onNavigate('grammar')}
                className="text-xs font-semibold hover:underline cursor-pointer"
                style={{ color: 'var(--accent-indigo)' }}
              >
                Explore Library →
              </button>
            </div>
            <div className="flex flex-col gap-3.5">
              {stats.jlpt_progress.map((row) => (
                <div key={row.level} className="flex items-center gap-4">
                  <div className="w-24 shrink-0 text-sm font-semibold">
                    {row.level}{' '}
                    <span className="font-normal text-xs" style={{ color: 'var(--text-muted)' }}>
                      {row.level === 'N5'
                        ? 'Basic'
                        : row.level === 'N4'
                          ? 'Elem.'
                          : row.level === 'N3'
                            ? 'Inter.'
                            : row.level === 'N2'
                              ? 'Upper'
                              : 'Adv.'}
                    </span>
                  </div>
                  <div
                    className="h-2.5 flex-1 overflow-hidden rounded-full"
                    style={{ backgroundColor: 'var(--bg-subtle)' }}
                  >
                    <div
                      className="h-full rounded-full transition-all duration-500"
                      style={{
                        width: `${Math.max(row.percent, row.learned > 0 ? 4 : 0)}%`,
                        backgroundColor: JLPT_COLORS[row.level] || 'var(--accent-indigo)',
                      }}
                    />
                  </div>
                  <div
                    className="font-mono-code w-20 text-right text-xs font-medium"
                    style={{ color: 'var(--text-secondary)' }}
                  >
                    {row.learned} / {row.total}
                  </div>
                </div>
              ))}
            </div>
          </div>

          {/* 24-Hour Review Forecast */}
          <div
            className="flex flex-col gap-4 rounded-2xl border p-6"
            style={{
              backgroundColor: 'var(--bg-surface)',
              borderColor: 'var(--border-subtle)',
            }}
          >
            <div className="flex items-center justify-between">
              <h2 className="text-base font-bold">24-Hour Review Forecast</h2>
              <span className="font-mono-code text-xs" style={{ color: 'var(--text-muted)' }}>
                {totalForecast} reviews scheduled
              </span>
            </div>

            <div className="flex h-36 items-end justify-between gap-2 pt-4">
              {stats.forecast_24h.map((bucket) => {
                const heightPx = Math.max(6, Math.round((bucket.count / maxForecast) * 96))
                const isNow = bucket.label === 'Now'
                return (
                  <div key={bucket.label} className="flex flex-1 flex-col items-center gap-1.5">
                    <span
                      className="font-mono-code text-[11px] font-semibold"
                      style={{
                        color: bucket.count > 0 ? 'var(--text-primary)' : 'var(--text-muted)',
                      }}
                    >
                      {bucket.count}
                    </span>
                    <div
                      className="w-full max-w-[44px] rounded-t-md transition-all duration-300"
                      style={{
                        height: `${heightPx}px`,
                        backgroundColor:
                          bucket.count === 0
                            ? 'var(--bg-subtle)'
                            : isNow
                              ? 'var(--accent-vermilion)'
                              : 'var(--accent-indigo)',
                      }}
                    />
                    <span className="font-mono-code text-[11px]" style={{ color: 'var(--text-muted)' }}>
                      {bucket.label}
                    </span>
                  </div>
                )
              })}
            </div>
          </div>
        </div>

        {/* Right Column (5/12): WaniKani Hub + SRS Distribution + Study Tools */}
        <div className="flex flex-col gap-6 lg:col-span-5">
          {/* WaniKani Hub Card */}
          <div
            className="flex flex-col gap-4 rounded-2xl border p-6"
            style={{
              backgroundColor: 'var(--bg-surface)',
              borderColor: stats.wanikani.connected ? 'var(--wk-pink)' : 'var(--border-subtle)',
            }}
          >
            <div className="flex items-center justify-between">
              <h2 className="text-base font-bold">WaniKani Integration</h2>
              {stats.wanikani.connected ? (
                <span
                  className="font-mono-code rounded-full px-2.5 py-1 text-[11px] font-bold text-white"
                  style={{ backgroundColor: 'var(--wk-pink)' }}
                >
                  Level {stats.wanikani.level}
                </span>
              ) : (
                <span
                  className="font-mono-code rounded-full px-2.5 py-1 text-[11px] font-semibold"
                  style={{ backgroundColor: 'var(--bg-subtle)', color: 'var(--text-muted)' }}
                >
                  Disconnected
                </span>
              )}
            </div>

            <p className="text-xs leading-relaxed" style={{ color: 'var(--text-secondary)' }}>
              {stats.wanikani.connected
                ? `Synced as ${stats.wanikani.username || 'Learner'}. Smart Furigana automatically hides readings on kanji you have passed (Guru+) in WaniKani.`
                : 'Sync your WaniKani API v2 token to automatically hide furigana on mastered kanji and unlock Kanji Readiness scores across all grammar sentences.'}
            </p>

            {stats.wanikani.connected ? (
              <div className="grid grid-cols-3 gap-2.5">
                <div
                  className="rounded-xl p-3"
                  style={{ backgroundColor: 'var(--wk-pink-soft)' }}
                >
                  <div className="font-mono-code text-lg font-bold" style={{ color: 'var(--wk-pink)' }}>
                    {stats.wanikani.kanji_known}
                  </div>
                  <div className="text-[11px]" style={{ color: 'var(--text-secondary)' }}>
                    Kanji Known
                  </div>
                </div>
                <div
                  className="rounded-xl p-3"
                  style={{ backgroundColor: 'var(--wk-purple-soft)' }}
                >
                  <div className="font-mono-code text-lg font-bold" style={{ color: 'var(--wk-purple)' }}>
                    {stats.wanikani.vocab_known}
                  </div>
                  <div className="text-[11px]" style={{ color: 'var(--text-secondary)' }}>
                    Vocab Known
                  </div>
                </div>
                <div
                  className="rounded-xl p-3"
                  style={{ backgroundColor: 'var(--accent-amber-soft)' }}
                >
                  <div className="font-mono-code text-lg font-bold" style={{ color: 'var(--accent-amber)' }}>
                    {stats.wanikani.wk_reviews_available}
                  </div>
                  <div className="text-[11px]" style={{ color: 'var(--text-secondary)' }}>
                    WK Reviews
                  </div>
                </div>
              </div>
            ) : (
              <div className="flex flex-wrap gap-2.5">
                <button
                  onClick={() => onNavigate('wanikani')}
                  className="flex-1 rounded-xl px-4 py-2.5 text-xs font-semibold text-white cursor-pointer"
                  style={{ backgroundColor: 'var(--wk-pink)' }}
                >
                  Connect API v2 Token
                </button>
                <button
                  onClick={handleQuickDemoWK}
                  disabled={syncingDemo}
                  className="flex items-center gap-1.5 rounded-xl border px-3.5 py-2.5 text-xs font-semibold cursor-pointer"
                  style={{
                    borderColor: 'var(--wk-pink)',
                    color: 'var(--wk-pink)',
                    backgroundColor: 'var(--wk-pink-soft)',
                  }}
                >
                  {syncingDemo ? (
                    <RefreshCw className="h-3.5 w-3.5 animate-spin" />
                  ) : (
                    <Sparkles className="h-3.5 w-3.5" />
                  )}
                  Try Demo Lv.14
                </button>
              </div>
            )}
          </div>

          {/* SRS Distribution */}
          <div
            className="flex flex-col gap-3.5 rounded-2xl border p-6"
            style={{
              backgroundColor: 'var(--bg-surface)',
              borderColor: 'var(--border-subtle)',
            }}
          >
            <h2 className="text-base font-bold">SRS Distribution</h2>
            <div className="flex flex-col gap-2.5">
              {stats.srs_breakdown.map((tier) => (
                <div key={tier.tier} className="flex items-center justify-between text-sm">
                  <div className="flex items-center gap-2.5">
                    <span
                      className="h-2.5 w-2.5 rounded-full"
                      style={{ backgroundColor: SRS_DOT_COLORS[tier.tier] || 'var(--accent-indigo)' }}
                    />
                    <span className="font-medium">
                      {tier.tier}{' '}
                      <span className="font-mono-code text-xs" style={{ color: 'var(--text-muted)' }}>
                        ({tier.range})
                      </span>
                    </span>
                  </div>
                  <span className="font-mono-code text-xs font-semibold" style={{ color: 'var(--text-secondary)' }}>
                    {tier.count} items
                  </span>
                </div>
              ))}
            </div>
          </div>
        </div>
      </section>

      {/* Study Analytics & Retention (90 Days) */}
      <StudyAnalyticsPanel onSelectGrammar={onSelectGrammar} onStartSession={onStartSession} />

      {/* Lower Two-Column Section: SRS Ergonomics & Backup + Bonus Learning Labs */}
      <section className="grid grid-cols-1 gap-6 lg:grid-cols-12">
        <div className="lg:col-span-7">
          {/* SRS Ergonomics & Data Backup */}
          <div
            className="flex flex-col gap-3.5 rounded-2xl border p-6"
            style={{
              backgroundColor: 'var(--bg-surface)',
              borderColor: 'var(--border-subtle)',
            }}
          >
            <div className="flex items-center justify-between">
              <h2 className="text-base font-bold">SRS Ergonomics &amp; Backup</h2>
              <span className="font-mono-code text-[11px]" style={{ color: 'var(--text-muted)' }}>
                Deck v{stats.onboarding.content_version}
              </span>
            </div>

            <div className="flex items-center justify-between gap-3 text-xs">
              <div>
                <div className="font-semibold">Vacation Mode</div>
                <div style={{ color: 'var(--text-muted)' }}>
                  Pauses due reviews &amp; preserves streak
                </div>
              </div>
              <button
                type="button"
                disabled={updatingSrs}
                onClick={handleToggleVacation}
                className="flex items-center gap-1.5 rounded-lg border px-3 py-1.5 text-xs font-semibold cursor-pointer"
                style={{
                  backgroundColor: stats.srs_settings.vacation_mode
                    ? 'var(--accent-amber-soft)'
                    : 'var(--bg-subtle)',
                  borderColor: stats.srs_settings.vacation_mode
                    ? 'var(--accent-amber)'
                    : 'var(--border-subtle)',
                  color: stats.srs_settings.vacation_mode
                    ? 'var(--accent-amber)'
                    : 'var(--text-secondary)',
                }}
              >
                <Palmtree className="h-3.5 w-3.5" />
                {stats.srs_settings.vacation_mode ? 'On' : 'Off'}
              </button>
            </div>

            <div className="flex items-center justify-between gap-3 text-xs">
              <span className="font-semibold">Ghost Review Intensity</span>
              <div
                className="flex rounded-lg border p-0.5"
                style={{ backgroundColor: 'var(--bg-canvas)', borderColor: 'var(--border-subtle)' }}
              >
                {(['minimal', 'standard', 'heavy'] as const).map((lvl) => {
                  const active = stats.srs_settings.ghost_intensity === lvl
                  return (
                    <button
                      key={lvl}
                      type="button"
                      disabled={updatingSrs}
                      onClick={() => handleGhostIntensity(lvl)}
                      className="rounded-md px-2.5 py-1 text-[11px] font-semibold capitalize cursor-pointer"
                      style={{
                        backgroundColor: active ? 'var(--accent-vermilion)' : 'transparent',
                        color: active ? '#ffffff' : 'var(--text-secondary)',
                      }}
                    >
                      {lvl}
                    </button>
                  )
                })}
              </div>
            </div>

            <div className="flex items-center justify-between gap-3 text-xs">
              <span className="font-semibold">Study Path Priority</span>
              <select
                value={stats.srs_settings.preferred_study_path}
                disabled={updatingSrs}
                onChange={(e) => handleStudyPathChange(e.target.value)}
                className="font-mono-code rounded-lg border px-2.5 py-1 text-xs outline-none"
                style={{
                  backgroundColor: 'var(--bg-canvas)',
                  borderColor: 'var(--border-subtle)',
                  color: 'var(--text-primary)',
                }}
              >
                {['JLPT Core', 'Genki I', 'Genki II', 'Tobira', 'Quartet'].map((p) => (
                  <option key={p} value={p}>
                    {p}
                  </option>
                ))}
              </select>
            </div>

            <div
              className="flex flex-wrap items-center gap-2 border-t pt-3"
              style={{ borderColor: 'var(--border-subtle)' }}
            >
              <button
                type="button"
                onClick={() => handleExport('json')}
                className="flex items-center gap-1 rounded-lg border px-2.5 py-1.5 text-xs font-semibold cursor-pointer"
                style={{ borderColor: 'var(--border-subtle)', backgroundColor: 'var(--bg-subtle)' }}
              >
                <Download className="h-3.5 w-3.5" />
                Export JSON
              </button>
              <button
                type="button"
                onClick={() => handleExport('csv')}
                className="flex items-center gap-1 rounded-lg border px-2.5 py-1.5 text-xs font-semibold cursor-pointer"
                style={{ borderColor: 'var(--border-subtle)', backgroundColor: 'var(--bg-subtle)' }}
              >
                <Download className="h-3.5 w-3.5" />
                Export CSV
              </button>
              <button
                type="button"
                onClick={() => importInputRef.current?.click()}
                className="flex items-center gap-1 rounded-lg border px-2.5 py-1.5 text-xs font-semibold cursor-pointer"
                style={{ borderColor: 'var(--border-subtle)', backgroundColor: 'var(--bg-subtle)' }}
              >
                <Upload className="h-3.5 w-3.5" />
                Import
              </button>
              <input
                ref={importInputRef}
                type="file"
                accept=".json,.csv,application/json,text/csv"
                onChange={handleImportFile}
                className="hidden"
              />
            </div>

            {dataMsg && (
              <div className="text-xs font-medium" style={{ color: 'var(--accent-matcha)' }}>
                {dataMsg}
              </div>
            )}
          </div>
        </div>

        {/* Quick Bonus Learning Labs */}
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:col-span-5 lg:grid-cols-1">
          <button
              onClick={() => onNavigate('conjugation')}
              className="flex flex-col gap-1 rounded-2xl border p-4 text-left transition-colors hover:opacity-90 cursor-pointer"
              style={{
                backgroundColor: 'var(--bg-surface)',
                borderColor: 'var(--border-subtle)',
              }}
            >
              <div className="flex items-center gap-2 text-sm font-bold" style={{ color: 'var(--accent-vermilion)' }}>
                <Swords className="h-4 w-4" />
                Conjugation Dojo
              </div>
              <span className="text-xs" style={{ color: 'var(--text-secondary)' }}>
                Drill て / ない / ば / potential forms with live IME
              </span>
            </button>
            <button
              onClick={() => onNavigate('reading')}
              className="flex flex-col gap-1 rounded-2xl border p-4 text-left transition-colors hover:opacity-90 cursor-pointer"
              style={{
                backgroundColor: 'var(--bg-surface)',
                borderColor: 'var(--border-subtle)',
              }}
            >
              <div className="flex items-center gap-2 text-sm font-bold" style={{ color: 'var(--accent-matcha)' }}>
                <BookMarked className="h-4 w-4" />
                Reading Lab
              </div>
              <span className="text-xs" style={{ color: 'var(--text-secondary)' }}>
                Analyze passages for grammar & WK kanji coverage
              </span>
            </button>
        </div>
      </section>
    </div>
  )
}

const MetricCard: React.FC<{
  label: string
  badge: string
  badgeColor: string
  value: string
  sub: string
}> = ({ label, badge, badgeColor, value, sub }) => (
  <div
    className="flex flex-col gap-1.5 rounded-2xl border p-5"
    style={{
      backgroundColor: 'var(--bg-surface)',
      borderColor: 'var(--border-subtle)',
    }}
  >
    <div className="flex items-center justify-between">
      <span className="text-xs font-semibold" style={{ color: 'var(--text-muted)' }}>
        {label}
      </span>
      <span className="font-mono-code text-[11px] font-semibold" style={{ color: badgeColor }}>
        {badge}
      </span>
    </div>
    <div className="text-3xl font-bold tracking-tight">{value}</div>
    <div className="text-xs" style={{ color: 'var(--text-secondary)' }}>
      {sub}
    </div>
  </div>
)

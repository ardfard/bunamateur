import React, { useEffect, useMemo, useState } from 'react'
import {
  Activity,
  ArrowUpRight,
  BarChart3,
  Calendar,
  CheckCircle2,
  Flame,
  Ghost,
  Target,
  Zap,
} from 'lucide-react'
import { api } from '../api'
import type {
  DailyActivityPoint,
  JlptAccuracyRow,
  ReviewMode,
  StudyAnalyticsResponse,
} from '../types'

interface StudyAnalyticsPanelProps {
  onSelectGrammar?: (id: number) => void
  onStartSession: (mode: ReviewMode) => void
}

const JLPT_ORDER = ['N5', 'N4', 'N3', 'N2', 'N1'] as const
const MONTH_NAMES = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec']
const WEEKDAY_LABELS = ['', 'Mon', '', 'Wed', '', 'Fri', '']

function formatPct(value: number): string {
  if (!Number.isFinite(value)) return '0%'
  return Number.isInteger(value) ? `${value}%` : `${value.toFixed(1)}%`
}

function getIntensityStyle(intensity: number): React.CSSProperties {
  const level = Math.max(0, Math.min(4, Math.round(intensity)))
  switch (level) {
    case 1:
      return { backgroundColor: 'var(--accent-matcha)', opacity: 0.28 }
    case 2:
      return { backgroundColor: 'var(--accent-matcha)', opacity: 0.52 }
    case 3:
      return { backgroundColor: 'var(--accent-matcha)', opacity: 0.78 }
    case 4:
      return { backgroundColor: 'var(--accent-vermilion)', opacity: 0.92 }
    default:
      return { backgroundColor: 'transparent', opacity: 0 }
  }
}

function getAccuracyTone(accuracy: number, reviews: number): {
  barColor: string
  textColor: string
  badgeBg: string
  label: string
} {
  if (reviews === 0) {
    return {
      barColor: 'var(--bg-subtle)',
      textColor: 'var(--text-muted)',
      badgeBg: 'var(--bg-subtle)',
      label: 'No data',
    }
  }
  if (accuracy < 60) {
    return {
      barColor: 'var(--accent-vermilion)',
      textColor: 'var(--accent-vermilion)',
      badgeBg: 'var(--accent-vermilion-soft)',
      label: 'Needs repair',
    }
  }
  if (accuracy < 75) {
    return {
      barColor: 'var(--accent-amber)',
      textColor: 'var(--accent-amber)',
      badgeBg: 'var(--accent-amber-soft)',
      label: 'Shaky',
    }
  }
  if (accuracy >= 85) {
    return {
      barColor: 'var(--accent-matcha)',
      textColor: 'var(--accent-matcha)',
      badgeBg: 'var(--accent-matcha-soft)',
      label: 'Strong',
    }
  }
  return {
    barColor: 'var(--accent-indigo)',
    textColor: 'var(--accent-indigo)',
    badgeBg: 'var(--accent-indigo-soft)',
    label: 'Steady',
  }
}

interface HeatmapColumn {
  key: string
  monthLabel: string | null
  days: (DailyActivityPoint | null)[]
}

function buildHeatmapColumns(points: DailyActivityPoint[]): HeatmapColumn[] {
  if (points.length === 0) {
    return Array.from({ length: 13 }, (_, idx) => ({
      key: `empty-${idx}`,
      monthLabel: null,
      days: Array.from({ length: 7 }, () => null),
    }))
  }

  const parsedFirst = new Date(`${points[0].date}T00:00:00Z`)
  const leadingPad = Number.isNaN(parsedFirst.getTime()) ? 0 : parsedFirst.getUTCDay()

  const padded: (DailyActivityPoint | null)[] = [
    ...Array.from({ length: leadingPad }, () => null),
    ...points,
  ]
  while (padded.length % 7 !== 0) {
    padded.push(null)
  }

  const columns: HeatmapColumn[] = []
  let lastMonth = -1

  for (let col = 0; col < padded.length; col += 7) {
    const slice = padded.slice(col, col + 7)
    let monthLabel: string | null = null
    for (const item of slice) {
      if (!item) continue
      const dt = new Date(`${item.date}T00:00:00Z`)
      if (!Number.isNaN(dt.getTime())) {
        const m = dt.getUTCMonth()
        if (m !== lastMonth) {
          monthLabel = MONTH_NAMES[m]
          lastMonth = m
        }
        break
      }
    }
    columns.push({
      key: slice.find((d) => d !== null)?.date || `col-${col}`,
      monthLabel,
      days: slice,
    })
  }

  return columns
}

export const StudyAnalyticsPanel: React.FC<StudyAnalyticsPanelProps> = ({
  onSelectGrammar,
  onStartSession,
}) => {
  const [data, setData] = useState<StudyAnalyticsResponse | null>(null)
  const [loading, setLoading] = useState(true)
  const [hoveredDay, setHoveredDay] = useState<DailyActivityPoint | null>(null)

  useEffect(() => {
    let cancelled = false
    api
      .getStudyAnalytics(91)
      .then((res) => {
        if (!cancelled) {
          setData(res)
        }
      })
      .catch(() => {
        // Silent failure — keep dashboard resilient if analytics endpoint is unavailable
      })
      .finally(() => {
        if (!cancelled) {
          setLoading(false)
        }
      })
    return () => {
      cancelled = true
    }
  }, [])

  const heatmapColumns = useMemo(
    () => buildHeatmapColumns(data?.heatmap ?? []),
    [data?.heatmap]
  )

  const jlptRows = useMemo<JlptAccuracyRow[]>(() => {
    if (!data) return []
    const byLevel = new Map<string, JlptAccuracyRow>()
    for (const row of data.accuracy_by_jlpt) {
      byLevel.set(row.jlpt_level.toUpperCase(), row)
    }
    return JLPT_ORDER.map(
      (level) =>
        byLevel.get(level) ?? {
          jlpt_level: level,
          reviews: 0,
          correct: 0,
          accuracy_pct: 0,
        }
    )
  }, [data])

  if (loading) {
    return (
      <section
        className="flex flex-col gap-5 rounded-2xl border p-6"
        style={{
          backgroundColor: 'var(--bg-surface)',
          borderColor: 'var(--border-subtle)',
        }}
        aria-busy="true"
        aria-label="Loading Study Analytics"
      >
        <div className="flex items-center justify-between">
          <div
            className="h-5 w-64 animate-pulse rounded-md"
            style={{ backgroundColor: 'var(--bg-subtle)' }}
          />
          <div
            className="h-4 w-32 animate-pulse rounded-md"
            style={{ backgroundColor: 'var(--bg-subtle)' }}
          />
        </div>
        <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
          {Array.from({ length: 4 }).map((_, i) => (
            <div
              key={i}
              className="h-20 animate-pulse rounded-xl"
              style={{ backgroundColor: 'var(--bg-subtle)' }}
            />
          ))}
        </div>
        <div
          className="h-36 animate-pulse rounded-xl"
          style={{ backgroundColor: 'var(--bg-subtle)' }}
        />
        <div className="grid grid-cols-1 gap-5 lg:grid-cols-2">
          <div
            className="h-44 animate-pulse rounded-xl"
            style={{ backgroundColor: 'var(--bg-subtle)' }}
          />
          <div
            className="h-44 animate-pulse rounded-xl"
            style={{ backgroundColor: 'var(--bg-subtle)' }}
          />
        </div>
      </section>
    )
  }

  if (!data) {
    return null
  }

  const leeches = data.top_leeches.slice(0, 6)
  const hasGhostLeeches = leeches.some((item) => item.ghost_level > 0)
  const windowDays = data.days || 91

  return (
    <section
      className="flex flex-col gap-6 rounded-2xl border p-6"
      style={{
        backgroundColor: 'var(--bg-surface)',
        borderColor: 'var(--border-subtle)',
      }}
    >
      {/* Header */}
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="flex items-center gap-2.5">
          <div
            className="flex h-9 w-9 items-center justify-center rounded-xl"
            style={{
              backgroundColor: 'var(--accent-matcha-soft)',
              color: 'var(--accent-matcha)',
            }}
          >
            <BarChart3 className="h-4.5 w-4.5" />
          </div>
          <div>
            <h2 className="text-base font-bold tracking-tight">
              Study Analytics &amp; Retention (90 Days)
            </h2>
            <p className="text-xs" style={{ color: 'var(--text-secondary)' }}>
              13-week review consistency, JLPT recall strength, and targeted leech repair
            </p>
          </div>
        </div>
        <span
          className="font-mono-code rounded-full border px-2.5 py-1 text-[11px] font-semibold"
          style={{
            backgroundColor: 'var(--bg-canvas)',
            borderColor: 'var(--border-subtle)',
            color: 'var(--text-secondary)',
          }}
        >
          {windowDays}-day rolling window
        </span>
      </div>

      {/* Summary KPI Strip */}
      <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
        <KpiTile
          icon={<Activity className="h-3.5 w-3.5" />}
          iconColor="var(--accent-indigo)"
          label="All-Time Reviews"
          value={data.total_reviews_all_time.toLocaleString()}
          sub="Total submitted answers"
        />
        <KpiTile
          icon={<Target className="h-3.5 w-3.5" />}
          iconColor={
            data.overall_accuracy_pct >= 85
              ? 'var(--accent-matcha)'
              : data.overall_accuracy_pct >= 75
                ? 'var(--accent-indigo)'
                : 'var(--accent-amber)'
          }
          label="Overall Accuracy"
          value={formatPct(data.overall_accuracy_pct)}
          sub="Lifetime recall rate"
        />
        <KpiTile
          icon={<Flame className="h-3.5 w-3.5" />}
          iconColor="var(--accent-vermilion)"
          label="Longest Streak"
          value={`${data.longest_streak_days}d`}
          sub={data.longest_streak_days === 1 ? '1 consecutive day' : 'Consecutive study days'}
        />
        <KpiTile
          icon={<Calendar className="h-3.5 w-3.5" />}
          iconColor="var(--accent-matcha)"
          label={`Active Days (${windowDays}d)`}
          value={`${data.active_days_in_window} / ${windowDays}`}
          sub={`${Math.round((data.active_days_in_window / Math.max(1, windowDays)) * 100)}% consistency`}
        />
      </div>

      {/* 13-Week Activity Heatmap */}
      <div
        className="flex flex-col gap-3 rounded-xl border p-4 sm:p-5"
        style={{
          backgroundColor: 'var(--bg-canvas)',
          borderColor: 'var(--border-subtle)',
        }}
      >
        <div className="flex flex-wrap items-center justify-between gap-2">
          <h3 className="text-sm font-bold">13-Week Activity Heatmap</h3>
          <div
            className="font-mono-code rounded-lg border px-2.5 py-1 text-[11px]"
            style={{
              backgroundColor: 'var(--bg-surface)',
              borderColor: 'var(--border-subtle)',
              color: hoveredDay ? 'var(--text-primary)' : 'var(--text-muted)',
            }}
          >
            {hoveredDay
              ? `${hoveredDay.date} • ${hoveredDay.total_reviews} ${
                  hoveredDay.total_reviews === 1 ? 'review' : 'reviews'
                } • ${formatPct(hoveredDay.accuracy_pct)} accuracy`
              : 'Hover a day cell to inspect date, reviews & accuracy'}
          </div>
        </div>

        <div className="overflow-x-auto pb-1">
          <div className="inline-flex min-w-full flex-col gap-1.5">
            {/* Month labels row */}
            <div className="flex items-end gap-1 pl-8">
              {heatmapColumns.map((col) => (
                <div
                  key={`month-${col.key}`}
                  className="font-mono-code w-4 shrink-0 text-[10px] sm:w-5"
                  style={{ color: 'var(--text-muted)' }}
                >
                  {col.monthLabel ?? ''}
                </div>
              ))}
            </div>

            {/* Grid body: weekday labels + week columns (7 rows each) */}
            <div className="flex gap-2">
              <div className="flex flex-col justify-between gap-1 py-0.5">
                {WEEKDAY_LABELS.map((label, idx) => (
                  <span
                    key={idx}
                    className="font-mono-code flex h-4 w-6 items-center text-[10px] sm:h-5"
                    style={{ color: 'var(--text-muted)' }}
                  >
                    {label}
                  </span>
                ))}
              </div>

              <div className="flex gap-1">
                {heatmapColumns.map((col) => (
                  <div key={col.key} className="flex flex-col gap-1">
                    {col.days.map((day, dayIdx) => {
                      if (!day) {
                        return (
                          <div
                            key={`${col.key}-empty-${dayIdx}`}
                            className="h-4 w-4 rounded-[4px] sm:h-5 sm:w-5"
                            aria-hidden="true"
                          />
                        )
                      }
                      const isHovered = hoveredDay?.date === day.date
                      const tooltip = `${day.date} • ${day.total_reviews} ${
                        day.total_reviews === 1 ? 'review' : 'reviews'
                      } • ${formatPct(day.accuracy_pct)} accuracy`
                      return (
                        <button
                          key={day.date}
                          type="button"
                          title={tooltip}
                          aria-label={tooltip}
                          onMouseEnter={() => setHoveredDay(day)}
                          onMouseLeave={() => setHoveredDay((prev) => (prev?.date === day.date ? null : prev))}
                          onFocus={() => setHoveredDay(day)}
                          onBlur={() => setHoveredDay((prev) => (prev?.date === day.date ? null : prev))}
                          className="relative h-4 w-4 overflow-hidden rounded-[4px] border transition-transform hover:scale-110 focus:outline-none sm:h-5 sm:w-5 cursor-pointer"
                          style={{
                            backgroundColor: 'var(--bg-subtle)',
                            borderColor: isHovered ? 'var(--text-primary)' : 'var(--border-subtle)',
                          }}
                        >
                          {day.intensity > 0 && (
                            <span
                              className="block h-full w-full"
                              style={getIntensityStyle(day.intensity)}
                            />
                          )}
                        </button>
                      )
                    })}
                  </div>
                ))}
              </div>
            </div>
          </div>
        </div>

        {/* Legend */}
        <div className="flex flex-wrap items-center justify-between gap-2 pt-1 text-[11px]">
          <span style={{ color: 'var(--text-muted)' }}>
            Peak intensity days highlight in vermilion
          </span>
          <div className="flex items-center gap-1.5" style={{ color: 'var(--text-muted)' }}>
            <span>Less</span>
            {[0, 1, 2, 3, 4].map((lvl) => (
              <span
                key={lvl}
                className="relative inline-block h-3.5 w-3.5 overflow-hidden rounded-[3px] border"
                style={{
                  backgroundColor: 'var(--bg-subtle)',
                  borderColor: 'var(--border-subtle)',
                }}
                title={`Intensity level ${lvl}`}
              >
                {lvl > 0 && <span className="block h-full w-full" style={getIntensityStyle(lvl)} />}
              </span>
            ))}
            <span>More</span>
          </div>
        </div>
      </div>

      {/* Two-Column Lower Grid: JLPT Accuracy + Top Leeches */}
      <div className="grid grid-cols-1 gap-6 lg:grid-cols-2">
        {/* Left: Accuracy by JLPT Level */}
        <div
          className="flex flex-col gap-4 rounded-xl border p-5"
          style={{
            backgroundColor: 'var(--bg-canvas)',
            borderColor: 'var(--border-subtle)',
          }}
        >
          <div className="flex items-center justify-between">
            <h3 className="text-sm font-bold">Accuracy by JLPT Level (N5–N1)</h3>
            <span className="font-mono-code text-[11px]" style={{ color: 'var(--text-muted)' }}>
              &lt;75% flag · ≥85% strong
            </span>
          </div>

          <div className="flex flex-col gap-3.5">
            {jlptRows.map((row) => {
              const tone = getAccuracyTone(row.accuracy_pct, row.reviews)
              const widthPct = row.reviews > 0 ? Math.max(4, Math.min(100, row.accuracy_pct)) : 0
              return (
                <div key={row.jlpt_level} className="flex flex-col gap-1.5">
                  <div className="flex items-center justify-between text-xs">
                    <div className="flex items-center gap-2">
                      <span className="font-mono-code font-bold">{row.jlpt_level}</span>
                      <span
                        className="rounded-full px-2 py-0.5 text-[10px] font-semibold"
                        style={{
                          backgroundColor: tone.badgeBg,
                          color: tone.textColor,
                        }}
                      >
                        {tone.label}
                      </span>
                    </div>
                    <div className="flex items-center gap-2.5">
                      <span className="font-mono-code text-[11px]" style={{ color: 'var(--text-muted)' }}>
                        {row.reviews.toLocaleString()} {row.reviews === 1 ? 'review' : 'reviews'}
                      </span>
                      <span
                        className="font-mono-code w-12 text-right text-xs font-bold"
                        style={{ color: tone.textColor }}
                      >
                        {row.reviews > 0 ? formatPct(row.accuracy_pct) : '—'}
                      </span>
                    </div>
                  </div>
                  <div
                    className="h-2.5 w-full overflow-hidden rounded-full"
                    style={{ backgroundColor: 'var(--bg-subtle)' }}
                  >
                    <div
                      className="h-full rounded-full transition-all duration-500"
                      style={{
                        width: `${widthPct}%`,
                        backgroundColor: tone.barColor,
                      }}
                    />
                  </div>
                </div>
              )
            })}
          </div>
        </div>

        {/* Right: Top Leeches to Repair */}
        <div
          className="flex flex-col justify-between gap-4 rounded-xl border p-5"
          style={{
            backgroundColor: 'var(--bg-canvas)',
            borderColor: 'var(--border-subtle)',
          }}
        >
          <div className="flex flex-col gap-3.5">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <div className="flex items-center gap-2">
                <h3 className="text-sm font-bold">Top Leeches to Repair</h3>
                {leeches.length > 0 && (
                  <span
                    className="font-mono-code rounded-full px-2 py-0.5 text-[10px] font-bold"
                    style={{
                      backgroundColor: 'var(--accent-vermilion-soft)',
                      color: 'var(--accent-vermilion)',
                    }}
                  >
                    {leeches.length} flagged
                  </span>
                )}
              </div>
              <div className="flex items-center gap-2">
                {hasGhostLeeches && (
                  <button
                    type="button"
                    onClick={() => onStartSession('ghost')}
                    className="flex items-center gap-1 rounded-lg border px-2.5 py-1 text-[11px] font-semibold transition-colors cursor-pointer"
                    style={{
                      backgroundColor: 'var(--accent-vermilion-soft)',
                      borderColor: 'var(--accent-vermilion)',
                      color: 'var(--accent-vermilion)',
                    }}
                  >
                    <Ghost className="h-3 w-3" />
                    Drill Ghosts
                  </button>
                )}
                <button
                  type="button"
                  onClick={() => onStartSession('cram')}
                  className="flex items-center gap-1 rounded-lg border px-2.5 py-1 text-[11px] font-semibold transition-colors cursor-pointer"
                  style={{
                    backgroundColor: 'var(--bg-surface)',
                    borderColor: 'var(--border-subtle)',
                    color: 'var(--text-primary)',
                  }}
                >
                  <Zap className="h-3 w-3" />
                  Cram Weak Points
                </button>
              </div>
            </div>

            {leeches.length === 0 ? (
              <div
                className="flex flex-col items-center justify-center gap-2.5 rounded-xl border border-dashed px-4 py-8 text-center"
                style={{
                  backgroundColor: 'var(--bg-surface)',
                  borderColor: 'var(--border-subtle)',
                }}
              >
                <div
                  className="flex h-10 w-10 items-center justify-center rounded-full"
                  style={{
                    backgroundColor: 'var(--accent-matcha-soft)',
                    color: 'var(--accent-matcha)',
                  }}
                >
                  <CheckCircle2 className="h-5 w-5" />
                </div>
                <div className="text-sm font-bold">Zero leeches right now!</div>
                <p className="max-w-xs text-xs leading-relaxed" style={{ color: 'var(--text-secondary)' }}>
                  Your retention is clean across all studied grammar points. Keep your daily review
                  streak going or sharpen weak nuances in Cram Mode.
                </p>
              </div>
            ) : (
              <div className="flex flex-col gap-2">
                {leeches.map((item) => {
                  const isSevere = item.accuracy_pct < 60
                  return (
                    <button
                      key={item.grammar_id}
                      type="button"
                      onClick={() => onSelectGrammar?.(item.grammar_id)}
                      className="group flex items-center justify-between gap-3 rounded-xl border px-3.5 py-2.5 text-left transition-colors hover:opacity-95 cursor-pointer"
                      style={{
                        backgroundColor: 'var(--bg-surface)',
                        borderColor: 'var(--border-subtle)',
                      }}
                    >
                      <div className="min-w-0 flex-1">
                        <div className="flex items-center gap-2">
                          <span className="font-jp truncate text-sm font-bold">
                            {item.title}
                          </span>
                          <span
                            className="font-mono-code shrink-0 rounded-md px-1.5 py-0.5 text-[10px] font-bold"
                            style={{
                              backgroundColor: 'var(--accent-indigo-soft)',
                              color: 'var(--accent-indigo)',
                            }}
                          >
                            {item.jlpt_level}
                          </span>
                          {item.ghost_level > 0 && (
                            <span
                              className="font-mono-code inline-flex shrink-0 items-center gap-0.5 rounded-md px-1.5 py-0.5 text-[10px] font-bold"
                              style={{
                                backgroundColor: 'var(--accent-vermilion-soft)',
                                color: 'var(--accent-vermilion)',
                              }}
                            >
                              <Ghost className="h-2.5 w-2.5" />
                              G{item.ghost_level}
                            </span>
                          )}
                        </div>
                        <div
                          className="truncate text-xs"
                          style={{ color: 'var(--text-secondary)' }}
                        >
                          {item.meaning}
                        </div>
                      </div>

                      <div className="flex shrink-0 items-center gap-2.5">
                        <div className="text-right">
                          <div
                            className="font-mono-code text-xs font-bold"
                            style={{
                              color: isSevere
                                ? 'var(--accent-vermilion)'
                                : 'var(--accent-amber)',
                            }}
                          >
                            {formatPct(item.accuracy_pct)}
                          </div>
                          <div
                            className="font-mono-code text-[10px]"
                            style={{ color: 'var(--text-muted)' }}
                          >
                            {item.incorrect_count}{' '}
                            {item.incorrect_count === 1 ? 'miss' : 'misses'}
                          </div>
                        </div>
                        <ArrowUpRight
                          className="h-3.5 w-3.5 transition-transform group-hover:translate-x-0.5 group-hover:-translate-y-0.5"
                          style={{ color: 'var(--text-muted)' }}
                        />
                      </div>
                    </button>
                  )
                })}
              </div>
            )}
          </div>
        </div>
      </div>
    </section>
  )
}

const KpiTile: React.FC<{
  icon: React.ReactNode
  iconColor: string
  label: string
  value: string
  sub: string
}> = ({ icon, iconColor, label, value, sub }) => (
  <div
    className="flex flex-col gap-1 rounded-xl border p-3.5"
    style={{
      backgroundColor: 'var(--bg-canvas)',
      borderColor: 'var(--border-subtle)',
    }}
  >
    <div className="flex items-center justify-between gap-2">
      <span className="text-[11px] font-semibold" style={{ color: 'var(--text-muted)' }}>
        {label}
      </span>
      <span style={{ color: iconColor }}>{icon}</span>
    </div>
    <div className="font-mono-code text-xl font-bold tracking-tight">{value}</div>
    <span className="text-[11px]" style={{ color: 'var(--text-secondary)' }}>
      {sub}
    </span>
  </div>
)

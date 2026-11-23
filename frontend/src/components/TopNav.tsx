import React from 'react'
import { Flame, Moon, Sun } from 'lucide-react'
import type { PageTab, WaniKaniStatus } from '../types'

interface TopNavProps {
  activeTab: PageTab
  onNavigate: (tab: PageTab) => void
  reviewsDue: number
  streakDays: number
  wanikani: WaniKaniStatus | null
  darkMode: boolean
  onToggleDark: () => void
}

const NAV_ITEMS: { id: PageTab; label: string }[] = [
  { id: 'dashboard', label: 'Dashboard' },
  { id: 'reviews', label: 'Reviews' },
  { id: 'grammar', label: 'Grammar' },
  { id: 'conjugation', label: 'Conjugation' },
  { id: 'reading', label: 'Reading Lab' },
  { id: 'wanikani', label: 'WaniKani' },
]

export const TopNav: React.FC<TopNavProps> = ({
  activeTab,
  onNavigate,
  reviewsDue,
  streakDays,
  wanikani,
  darkMode,
  onToggleDark,
}) => {
  return (
    <header
      className="sticky top-0 z-30 w-full border-b transition-colors"
      style={{
        backgroundColor: 'var(--bg-surface)',
        borderColor: 'var(--border-subtle)',
      }}
    >
      <div className="mx-auto flex h-[68px] max-w-[1360px] items-center justify-between px-4 sm:px-8">
        {/* Brand Hanko + Wordmark */}
        <button
          onClick={() => onNavigate('dashboard')}
          className="flex items-center gap-3 cursor-pointer"
        >
          <div
            className="flex h-[34px] w-[34px] items-center justify-center rounded-lg font-jp text-lg font-bold text-white shadow-xs"
            style={{ backgroundColor: 'var(--accent-vermilion)' }}
          >
            文
          </div>
          <div className="flex flex-col text-left">
            <span className="text-[17px] font-bold leading-tight tracking-tight">
              BunAmateur
            </span>
            <span
              className="font-mono-code text-[10px] tracking-wider uppercase"
              style={{ color: 'var(--text-muted)' }}
            >
              文アマ • SRS
            </span>
          </div>
        </button>

        {/* Center Navigation */}
        <nav className="hidden items-center gap-1 md:flex">
          {NAV_ITEMS.map((item) => {
            const active = activeTab === item.id
            return (
              <button
                key={item.id}
                onClick={() => onNavigate(item.id)}
                className="relative flex items-center gap-1.5 rounded-lg px-3.5 py-2 text-sm font-medium transition-colors cursor-pointer"
                style={{
                  color: active ? 'var(--accent-vermilion)' : 'var(--text-secondary)',
                  backgroundColor: active ? 'var(--accent-vermilion-soft)' : 'transparent',
                  fontWeight: active ? 600 : 500,
                }}
              >
                <span>{item.label}</span>
                {item.id === 'reviews' && reviewsDue > 0 && (
                  <span
                    className="font-mono-code rounded-full px-1.5 py-0.5 text-[10px] font-bold text-white"
                    style={{ backgroundColor: 'var(--accent-vermilion)' }}
                  >
                    {reviewsDue}
                  </span>
                )}
              </button>
            )
          })}
        </nav>

        {/* Right Status Pills */}
        <div className="flex items-center gap-2.5">
          <button
            onClick={() => onNavigate('wanikani')}
            className="flex items-center gap-1.5 rounded-full px-3 py-1.5 text-xs font-semibold transition-opacity hover:opacity-85 cursor-pointer"
            style={{
              backgroundColor: wanikani?.connected ? 'var(--wk-pink-soft)' : 'var(--bg-subtle)',
              color: wanikani?.connected ? 'var(--wk-pink)' : 'var(--text-secondary)',
            }}
            title="WaniKani Integration Settings"
          >
            <span
              className="h-2 w-2 rounded-full"
              style={{
                backgroundColor: wanikani?.connected ? 'var(--wk-pink)' : 'var(--text-muted)',
              }}
            />
            <span>
              {wanikani?.connected
                ? `WK Lv.${wanikani.level} Synced`
                : 'Connect WaniKani'}
            </span>
          </button>

          <div
            className="hidden sm:flex items-center gap-1.5 rounded-full px-3 py-1.5 font-mono-code text-xs font-semibold"
            style={{
              backgroundColor: 'var(--accent-amber-soft)',
              color: 'var(--accent-amber)',
            }}
            title="Daily Study Streak"
          >
            <Flame className="h-3.5 w-3.5" />
            <span>{streakDays}d</span>
          </div>

          <button
            onClick={onToggleDark}
            className="flex h-9 w-9 items-center justify-center rounded-lg border transition-colors cursor-pointer"
            style={{
              borderColor: 'var(--border-subtle)',
              backgroundColor: 'var(--bg-subtle)',
              color: 'var(--text-secondary)',
            }}
            title={darkMode ? 'Switch to Washi Light Mode' : 'Switch to Sumi Dark Mode'}
          >
            {darkMode ? <Sun className="h-4 w-4" /> : <Moon className="h-4 w-4" />}
          </button>
        </div>
      </div>

      {/* Mobile Nav Strip */}
      <div
        className="flex items-center gap-1 overflow-x-auto border-t px-3 py-1.5 md:hidden"
        style={{ borderColor: 'var(--border-subtle)' }}
      >
        {NAV_ITEMS.map((item) => {
          const active = activeTab === item.id
          return (
            <button
              key={item.id}
              onClick={() => onNavigate(item.id)}
              className="shrink-0 rounded-md px-3 py-1.5 text-xs font-medium cursor-pointer"
              style={{
                color: active ? 'var(--accent-vermilion)' : 'var(--text-secondary)',
                backgroundColor: active ? 'var(--accent-vermilion-soft)' : 'transparent',
              }}
            >
              {item.label}
            </button>
          )
        })}
      </div>
    </header>
  )
}

import React from 'react'
import { Flame, LogOut, Moon, Settings, Sun } from 'lucide-react'
import type { AuthUser, PageTab, WaniKaniStatus } from '../types'
import { buildRoutePath } from '../utils/router'

interface TopNavProps {
  activeTab: PageTab
  onNavigate: (tab: PageTab) => void
  reviewsDue: number
  streakDays: number
  wanikani: WaniKaniStatus | null
  user: AuthUser
  onLogout: () => void
  darkMode: boolean
  onToggleDark: () => void
  onOpenAccount?: () => void
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
  user,
  onLogout,
  darkMode,
  onToggleDark,
  onOpenAccount,
}) => {
  const handleNavClick = (e: React.MouseEvent<HTMLAnchorElement>, tab: PageTab) => {
    if (
      e.defaultPrevented ||
      e.button !== 0 ||
      e.metaKey ||
      e.ctrlKey ||
      e.shiftKey ||
      e.altKey
    ) {
      return
    }
    e.preventDefault()
    onNavigate(tab)
  }

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
        <a
          href={buildRoutePath({ tab: 'dashboard' })}
          onClick={(e) => handleNavClick(e, 'dashboard')}
          className="flex items-center gap-3 cursor-pointer no-underline"
          style={{ color: 'inherit' }}
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
        </a>

        {/* Center Navigation */}
        <nav className="hidden items-center gap-1 md:flex" aria-label="Primary">
          {NAV_ITEMS.map((item) => {
            const active = activeTab === item.id
            return (
              <a
                key={item.id}
                href={buildRoutePath({ tab: item.id })}
                onClick={(e) => handleNavClick(e, item.id)}
                aria-current={active ? 'page' : undefined}
                className="relative flex items-center gap-1.5 rounded-lg px-3.5 py-2 text-sm font-medium transition-colors cursor-pointer no-underline"
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
              </a>
            )
          })}
        </nav>

        {/* Right Status Pills */}
        <div className="flex items-center gap-2.5">
          <a
            href={buildRoutePath({ tab: 'wanikani' })}
            onClick={(e) => handleNavClick(e, 'wanikani')}
            className="flex items-center gap-1.5 rounded-full px-3 py-1.5 text-xs font-semibold transition-opacity hover:opacity-85 cursor-pointer no-underline"
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
          </a>

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
            type="button"
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

          <button
            type="button"
            onClick={() => onOpenAccount?.()}
            className="hidden lg:flex items-center gap-1.5 rounded-full border px-2.5 py-1 text-xs font-medium transition-opacity hover:opacity-80 cursor-pointer"
            style={{
              borderColor: 'var(--border-subtle)',
              backgroundColor: 'var(--bg-subtle)',
              color: 'var(--text-secondary)',
            }}
            title={`Account Settings (${user.email})`}
            aria-label={`Open account settings for ${user.username}`}
          >
            {user.avatar_url ? (
              <img
                src={user.avatar_url}
                alt={user.username}
                referrerPolicy="no-referrer"
                className="h-5 w-5 rounded-full object-cover"
              />
            ) : (
              <Settings className="h-3.5 w-3.5" style={{ color: 'var(--text-muted)' }} />
            )}
            <span>{user.username}</span>
          </button>

          <button
            type="button"
            onClick={() => onOpenAccount?.()}
            className="flex lg:hidden h-9 w-9 items-center justify-center rounded-lg border transition-colors hover:opacity-85 cursor-pointer"
            style={{
              borderColor: 'var(--border-subtle)',
              backgroundColor: 'var(--bg-subtle)',
              color: 'var(--text-secondary)',
            }}
            title={`Account Settings (${user.username})`}
            aria-label="Account Settings"
          >
            <Settings className="h-4 w-4" />
          </button>

          <button
            type="button"
            onClick={onLogout}
            className="flex h-9 w-9 items-center justify-center rounded-lg border transition-colors cursor-pointer"
            style={{
              borderColor: 'var(--border-subtle)',
              backgroundColor: 'var(--bg-subtle)',
              color: 'var(--text-secondary)',
            }}
            title={`Sign out (${user.username})`}
          >
            <LogOut className="h-4 w-4" />
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
            <a
              key={item.id}
              href={buildRoutePath({ tab: item.id })}
              onClick={(e) => handleNavClick(e, item.id)}
              aria-current={active ? 'page' : undefined}
              className="shrink-0 rounded-md px-3 py-1.5 text-xs font-medium cursor-pointer no-underline"
              style={{
                color: active ? 'var(--accent-vermilion)' : 'var(--text-secondary)',
                backgroundColor: active ? 'var(--accent-vermilion-soft)' : 'transparent',
              }}
            >
              {item.label}
            </a>
          )
        })}
      </div>
    </header>
  )
}

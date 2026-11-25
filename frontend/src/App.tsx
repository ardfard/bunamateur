import React, { useCallback, useEffect, useMemo, useState } from 'react'
import { ApiError, api } from './api'
import { GrammarDetailModal } from './components/GrammarDetailModal'
import { TopNav } from './components/TopNav'
import { AuthPage } from './pages/AuthPage'
import { ConjugationPage } from './pages/ConjugationPage'
import { DashboardPage } from './pages/DashboardPage'
import { GrammarPage } from './pages/GrammarPage'
import { ReadingLabPage } from './pages/ReadingLabPage'
import { ReviewPage } from './pages/ReviewPage'
import { WaniKaniPage } from './pages/WaniKaniPage'
import type { AuthUser, DashboardStats, FuriganaMode, PageTab, ReviewMode } from './types'

export const App: React.FC = () => {
  const [user, setUser] = useState<AuthUser | null>(null)
  const [authLoading, setAuthLoading] = useState(true)
  const [activeTab, setActiveTab] = useState<PageTab>('dashboard')
  const [reviewMode, setReviewMode] = useState<ReviewMode>('review')
  const [stats, setStats] = useState<DashboardStats | null>(null)
  const [selectedGrammarId, setSelectedGrammarId] = useState<number | null>(null)
  const [refreshKey, setRefreshKey] = useState(0)

  const [darkMode, setDarkMode] = useState<boolean>(() => {
    if (typeof window === 'undefined') return false
    return localStorage.getItem('bunamateur-theme') === 'dark'
  })

  const [furiganaMode, setFuriganaMode] = useState<FuriganaMode>('smart')

  useEffect(() => {
    const root = document.documentElement
    if (darkMode) {
      root.classList.add('dark')
      localStorage.setItem('bunamateur-theme', 'dark')
    } else {
      root.classList.remove('dark')
      localStorage.setItem('bunamateur-theme', 'light')
    }
  }, [darkMode])

  useEffect(() => {
    let active = true
    api
      .authMe()
      .then((res) => {
        if (active) setUser(res.user)
      })
      .catch(() => {
        if (active) setUser(null)
      })
      .finally(() => {
        if (active) setAuthLoading(false)
      })
    return () => {
      active = false
    }
  }, [])

  const loadStats = useCallback(async () => {
    if (!user) return
    try {
      const data = await api.getDashboard()
      setStats(data)
      if (!data.wanikani.smart_furigana) {
        setFuriganaMode((prev) => (prev === 'smart' ? 'all' : prev))
      }
    } catch (e) {
      if (e instanceof ApiError && e.status === 401) {
        setUser(null)
        setStats(null)
        return
      }
      console.error('Failed to load dashboard:', e)
    }
  }, [user])

  useEffect(() => {
    if (user) {
      loadStats()
    } else {
      setStats(null)
    }
  }, [user, loadStats])

  const handleRefreshAll = useCallback(() => {
    loadStats()
    setRefreshKey((k) => k + 1)
  }, [loadStats])

  const handleLogout = useCallback(async () => {
    try {
      await api.logout()
    } catch {
      // Clear local session state even if cookie was already expired
    }
    setUser(null)
    setStats(null)
    setSelectedGrammarId(null)
    setActiveTab('dashboard')
  }, [])

  const knownKanji = useMemo(() => {
    return new Set(stats?.wanikani.known_kanji_list || [])
  }, [stats])

  const handleStartSession = (mode: ReviewMode) => {
    setReviewMode(mode)
    setActiveTab('reviews')
  }

  if (authLoading) {
    return (
      <div
        className="min-h-screen flex items-center justify-center text-sm"
        style={{ backgroundColor: 'var(--bg-canvas)', color: 'var(--text-muted)' }}
      >
        Loading BunAmateur...
      </div>
    )
  }

  if (!user) {
    return (
      <AuthPage
        darkMode={darkMode}
        onToggleDark={() => setDarkMode((d) => !d)}
        onAuthenticated={(u) => {
          setUser(u)
          setActiveTab('dashboard')
        }}
      />
    )
  }

  return (
    <div className="min-h-screen flex flex-col">
      <TopNav
        activeTab={activeTab}
        onNavigate={setActiveTab}
        reviewsDue={stats?.reviews_due ?? 0}
        streakDays={stats?.streak_days ?? 0}
        wanikani={stats?.wanikani ?? null}
        user={user}
        onLogout={handleLogout}
        darkMode={darkMode}
        onToggleDark={() => setDarkMode((d) => !d)}
      />

      <main className="flex-1">
        {activeTab === 'dashboard' && stats && (
          <DashboardPage
            stats={stats}
            onStartSession={handleStartSession}
            onNavigate={setActiveTab}
            onRefresh={handleRefreshAll}
          />
        )}

        {activeTab === 'reviews' && (
          <ReviewPage
            initialMode={reviewMode}
            furiganaMode={furiganaMode}
            onChangeFuriganaMode={setFuriganaMode}
            knownKanji={knownKanji}
            highlightKnown={stats?.wanikani.highlight_known_kanji ?? true}
            onSessionComplete={handleRefreshAll}
          />
        )}

        {activeTab === 'grammar' && (
          <GrammarPage
            onSelectGrammar={setSelectedGrammarId}
            refreshKey={refreshKey}
          />
        )}

        {activeTab === 'conjugation' && <ConjugationPage />}

        {activeTab === 'reading' && (
          <ReadingLabPage
            furiganaMode={furiganaMode}
            knownKanji={knownKanji}
            highlightKnown={stats?.wanikani.highlight_known_kanji ?? true}
            onSelectGrammar={setSelectedGrammarId}
          />
        )}

        {activeTab === 'wanikani' && stats && (
          <WaniKaniPage
            status={stats.wanikani}
            knownKanji={knownKanji}
            onUpdated={handleRefreshAll}
          />
        )}
      </main>

      <GrammarDetailModal
        grammarId={selectedGrammarId}
        onClose={() => setSelectedGrammarId(null)}
        onChanged={handleRefreshAll}
        furiganaMode={furiganaMode}
        knownKanji={knownKanji}
        highlightKnown={stats?.wanikani.highlight_known_kanji ?? true}
      />
    </div>
  )
}

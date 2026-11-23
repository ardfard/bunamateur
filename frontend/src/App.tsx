import React, { useCallback, useEffect, useMemo, useState } from 'react'
import { api } from './api'
import { GrammarDetailModal } from './components/GrammarDetailModal'
import { TopNav } from './components/TopNav'
import { ConjugationPage } from './pages/ConjugationPage'
import { DashboardPage } from './pages/DashboardPage'
import { GrammarPage } from './pages/GrammarPage'
import { ReadingLabPage } from './pages/ReadingLabPage'
import { ReviewPage } from './pages/ReviewPage'
import { WaniKaniPage } from './pages/WaniKaniPage'
import type { DashboardStats, FuriganaMode, PageTab, ReviewMode } from './types'

export const App: React.FC = () => {
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

  const loadStats = useCallback(async () => {
    try {
      const data = await api.getDashboard()
      setStats(data)
      if (!data.wanikani.smart_furigana) {
        setFuriganaMode((prev) => (prev === 'smart' ? 'all' : prev))
      }
    } catch (e) {
      console.error('Failed to load dashboard:', e)
    }
  }, [])

  useEffect(() => {
    loadStats()
  }, [loadStats])

  const handleRefreshAll = useCallback(() => {
    loadStats()
    setRefreshKey((k) => k + 1)
  }, [loadStats])

  const knownKanji = useMemo(() => {
    return new Set(stats?.wanikani.known_kanji_list || [])
  }, [stats])

  const handleStartSession = (mode: ReviewMode) => {
    setReviewMode(mode)
    setActiveTab('reviews')
  }

  return (
    <div className="min-h-screen flex flex-col">
      <TopNav
        activeTab={activeTab}
        onNavigate={setActiveTab}
        reviewsDue={stats?.reviews_due ?? 0}
        streakDays={stats?.streak_days ?? 0}
        wanikani={stats?.wanikani ?? null}
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

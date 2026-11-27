import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { ApiError, api } from './api'
import { AccountSettingsModal, type SettingsTab } from './components/AccountSettingsModal'
import { EmailVerificationBanner } from './components/EmailVerificationBanner'
import { GrammarDetailModal } from './components/GrammarDetailModal'
import { useToast } from './components/ToastContext'
import { TopNav } from './components/TopNav'
import { AuthPage } from './pages/AuthPage'
import { ConjugationPage } from './pages/ConjugationPage'
import { DashboardPage } from './pages/DashboardPage'
import { GrammarPage } from './pages/GrammarPage'
import { ReadingLabPage } from './pages/ReadingLabPage'
import { ReviewPage } from './pages/ReviewPage'
import type { AuthUser, DashboardStats, FuriganaMode, PageTab, ReviewMode } from './types'
import { buildRoutePath, navigateTo, parseRoute } from './utils/router'

function getInitialRoute() {
  if (typeof window === 'undefined') {
    return { tab: 'dashboard' as PageTab, grammarSlug: null as string | null, reviewMode: 'review' as ReviewMode }
  }
  const parsed = parseRoute(window.location.pathname, window.location.search)
  return {
    tab: parsed.tab,
    grammarSlug: parsed.grammarSlug ?? null,
    reviewMode: parsed.reviewMode ?? 'review',
  }
}

export const App: React.FC = () => {
  const initialRoute = useMemo(() => getInitialRoute(), [])
  const { toast } = useToast()

  const [user, setUser] = useState<AuthUser | null>(null)
  const [authLoading, setAuthLoading] = useState(true)
  const [activeTab, setActiveTab] = useState<PageTab>(
    initialRoute.tab === 'wanikani' ? 'dashboard' : initialRoute.tab
  )
  const [reviewMode, setReviewMode] = useState<ReviewMode>(initialRoute.reviewMode)
  const [stats, setStats] = useState<DashboardStats | null>(null)
  const [selectedGrammarId, setSelectedGrammarId] = useState<number | null>(null)
  const [pendingGrammarSlug, setPendingGrammarSlug] = useState<string | null>(initialRoute.grammarSlug)
  const [accountModalOpen, setAccountModalOpen] = useState(initialRoute.tab === 'wanikani')
  const [accountModalTab, setAccountModalTab] = useState<SettingsTab>(
    initialRoute.tab === 'wanikani' ? 'wanikani' : 'profile'
  )
  const [refreshKey, setRefreshKey] = useState(0)

  const slugCacheRef = useRef<Map<string, number>>(new Map())
  const verifyEmailAttemptedRef = useRef(false)

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

  // Handle /verify-email?token=... on mount (works whether signed in or signed out)
  useEffect(() => {
    if (typeof window === 'undefined') return
    if (window.location.pathname !== '/verify-email') return
    const params = new URLSearchParams(window.location.search)
    const token = params.get('token')?.trim()
    if (!token || verifyEmailAttemptedRef.current) return
    verifyEmailAttemptedRef.current = true

    api
      .verifyEmail(token)
      .then((res) => {
        toast.success(res.message || 'Email verified successfully.')
        window.history.replaceState({}, '', '/')
        api
          .authMe()
          .then((r) => setUser(r.user))
          .catch(() => {})
      })
      .catch((err) => {
        const msg = err instanceof Error ? err.message : ''
        toast.error(msg || 'Verification link is invalid or expired.')
        window.history.replaceState({}, '', '/')
      })
  }, [toast])

  // Sync browser Back/Forward navigation
  useEffect(() => {
    const handlePopState = () => {
      const route = parseRoute(window.location.pathname, window.location.search)
      if (route.tab === 'wanikani') {
        setActiveTab('dashboard')
        setAccountModalTab('wanikani')
        setAccountModalOpen(true)
        setSelectedGrammarId(null)
        setPendingGrammarSlug(null)
        return
      }
      setActiveTab(route.tab)
      if (route.tab === 'reviews') {
        setReviewMode(route.reviewMode ?? 'review')
      }
      if (route.tab === 'grammar' && route.grammarSlug) {
        const cachedId = slugCacheRef.current.get(route.grammarSlug)
        if (cachedId !== undefined) {
          setSelectedGrammarId(cachedId)
          setPendingGrammarSlug(null)
        } else {
          setPendingGrammarSlug(route.grammarSlug)
        }
      } else {
        setSelectedGrammarId(null)
        setPendingGrammarSlug(null)
      }
    }

    window.addEventListener('popstate', handlePopState)
    return () => window.removeEventListener('popstate', handlePopState)
  }, [])

  // Resolve deep-linked /grammar/:slug into grammar ID once authenticated
  useEffect(() => {
    if (!user || !pendingGrammarSlug) return
    let active = true

    const cachedId = slugCacheRef.current.get(pendingGrammarSlug)
    if (cachedId !== undefined) {
      setSelectedGrammarId(cachedId)
      setPendingGrammarSlug(null)
      return
    }

    api
      .getGrammarList()
      .then((list) => {
        if (!active) return
        for (const item of list) {
          slugCacheRef.current.set(item.slug, item.id)
        }
        const match = list.find((item) => item.slug === pendingGrammarSlug)
        if (match) {
          setSelectedGrammarId(match.id)
        } else {
          toast.error(`Grammar point "${pendingGrammarSlug}" was not found.`)
          navigateTo(buildRoutePath({ tab: 'grammar', grammarSlug: null }), true)
        }
        setPendingGrammarSlug(null)
      })
      .catch((e) => {
        if (!active) return
        console.error('Failed to resolve grammar slug:', e)
        toast.error('Failed to load grammar point.')
        setPendingGrammarSlug(null)
      })

    return () => {
      active = false
    }
  }, [user, pendingGrammarSlug, toast])

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
      toast.error('Could not refresh dashboard stats.')
    }
  }, [user, toast])

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

  const openAccountModal = useCallback((tab: SettingsTab = 'profile') => {
    setAccountModalTab(tab)
    setAccountModalOpen(true)
  }, [])

  const handleCloseAccountModal = useCallback(() => {
    setAccountModalOpen(false)
    if (typeof window !== 'undefined' && window.location.pathname.replace(/\/+$/, '') === '/wanikani') {
      navigateTo('/', true)
    }
  }, [])

  const handleNavigate = useCallback((tab: PageTab) => {
    if (tab === 'wanikani') {
      openAccountModal('wanikani')
      return
    }
    setActiveTab(tab)
    setSelectedGrammarId(null)
    setPendingGrammarSlug(null)
    navigateTo(buildRoutePath({ tab }))
  }, [openAccountModal])

  const handleStartSession = useCallback((mode: ReviewMode) => {
    setReviewMode(mode)
    setActiveTab('reviews')
    setSelectedGrammarId(null)
    setPendingGrammarSlug(null)
    navigateTo(buildRoutePath({ tab: 'reviews', reviewMode: mode }))
  }, [])

  const handleReviewModeChange = useCallback((mode: ReviewMode) => {
    setReviewMode(mode)
    navigateTo(buildRoutePath({ tab: 'reviews', reviewMode: mode }), true)
  }, [])

  const handleSelectGrammar = useCallback((id: number, slug?: string) => {
    setSelectedGrammarId(id)
    setPendingGrammarSlug(null)
    if (slug) {
      slugCacheRef.current.set(slug, id)
      navigateTo(buildRoutePath({ tab: 'grammar', grammarSlug: slug }))
    }
  }, [])

  const handleCloseGrammarModal = useCallback(() => {
    setSelectedGrammarId(null)
    setPendingGrammarSlug(null)
    if (typeof window !== 'undefined' && window.location.pathname.startsWith('/grammar/')) {
      navigateTo(buildRoutePath({ tab: 'grammar', grammarSlug: null }))
    }
  }, [])

  const handleLogout = useCallback(async () => {
    try {
      await api.logout()
    } catch {
      // Clear local session state even if cookie was already expired
    }
    setUser(null)
    setStats(null)
    setAccountModalOpen(false)
    setSelectedGrammarId(null)
    setPendingGrammarSlug(null)
    setActiveTab('dashboard')
    navigateTo('/', true)
    toast.info('Signed out.')
  }, [toast])

  const knownKanji = useMemo(() => {
    return new Set(stats?.wanikani.known_kanji_list || [])
  }, [stats])

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
          const route = parseRoute(window.location.pathname, window.location.search)
          if (route.tab === 'wanikani') {
            setActiveTab('dashboard')
            setAccountModalTab('wanikani')
            setAccountModalOpen(true)
          } else {
            setActiveTab(route.tab)
          }
          if (route.reviewMode) setReviewMode(route.reviewMode)
          if (route.grammarSlug) setPendingGrammarSlug(route.grammarSlug)
        }}
      />
    )
  }

  return (
    <div className="min-h-screen flex flex-col">
      <TopNav
        activeTab={activeTab}
        onNavigate={handleNavigate}
        reviewsDue={stats?.reviews_due ?? 0}
        streakDays={stats?.streak_days ?? 0}
        wanikani={stats?.wanikani ?? null}
        user={user}
        onOpenAccount={openAccountModal}
        onLogout={handleLogout}
        darkMode={darkMode}
        onToggleDark={() => setDarkMode((d) => !d)}
      />

      <EmailVerificationBanner
        user={user}
        onVerifiedRefresh={() => {
          void api
            .authMe()
            .then((r) => setUser(r.user))
            .catch(() => {})
        }}
      />

      <main className="flex-1">
        {activeTab === 'dashboard' && stats && (
          <DashboardPage
            stats={stats}
            onStartSession={handleStartSession}
            onNavigate={handleNavigate}
            onSelectGrammar={handleSelectGrammar}
            onRefresh={handleRefreshAll}
          />
        )}

        {activeTab === 'reviews' && (
          <ReviewPage
            initialMode={reviewMode}
            onModeChange={handleReviewModeChange}
            furiganaMode={furiganaMode}
            onChangeFuriganaMode={setFuriganaMode}
            knownKanji={knownKanji}
            highlightKnown={stats?.wanikani.highlight_known_kanji ?? true}
            onSessionComplete={handleRefreshAll}
          />
        )}

        {activeTab === 'grammar' && (
          <GrammarPage
            onSelectGrammar={handleSelectGrammar}
            refreshKey={refreshKey}
          />
        )}

        {activeTab === 'conjugation' && <ConjugationPage />}

        {activeTab === 'reading' && (
          <ReadingLabPage
            furiganaMode={furiganaMode}
            knownKanji={knownKanji}
            highlightKnown={stats?.wanikani.highlight_known_kanji ?? true}
            onSelectGrammar={handleSelectGrammar}
          />
        )}
      </main>

      <GrammarDetailModal
        grammarId={selectedGrammarId}
        onClose={handleCloseGrammarModal}
        onChanged={handleRefreshAll}
        furiganaMode={furiganaMode}
        knownKanji={knownKanji}
        highlightKnown={stats?.wanikani.highlight_known_kanji ?? true}
      />

      <AccountSettingsModal
        open={accountModalOpen}
        initialTab={accountModalTab}
        onClose={handleCloseAccountModal}
        onUserUpdated={(updatedUser) => setUser(updatedUser)}
        wanikaniStatus={stats?.wanikani ?? null}
        knownKanji={knownKanji}
        onWaniKaniUpdated={handleRefreshAll}
      />
    </div>
  )
}

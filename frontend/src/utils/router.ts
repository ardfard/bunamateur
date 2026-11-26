import type { PageTab, ReviewMode } from '../types'

export interface AppRoute {
  tab: PageTab
  grammarSlug?: string | null
  reviewMode?: ReviewMode
}

const VALID_REVIEW_MODES = new Set<ReviewMode>(['review', 'lesson', 'cram', 'ghost'])

function safeDecodeURIComponent(value: string): string {
  try {
    return decodeURIComponent(value)
  } catch {
    return value
  }
}

export function parseRoute(pathname: string, search = ''): AppRoute {
  const normalized = pathname.replace(/\/+$/, '') || '/'

  if (normalized === '/') {
    return { tab: 'dashboard' }
  }

  if (normalized === '/reviews') {
    const params = new URLSearchParams(search)
    const rawMode = params.get('mode')
    if (rawMode && VALID_REVIEW_MODES.has(rawMode as ReviewMode)) {
      return { tab: 'reviews', reviewMode: rawMode as ReviewMode }
    }
    return { tab: 'reviews' }
  }

  if (normalized === '/grammar') {
    return { tab: 'grammar', grammarSlug: null }
  }

  if (normalized.startsWith('/grammar/')) {
    const rawSlug = normalized.slice('/grammar/'.length)
    if (rawSlug && !rawSlug.includes('/')) {
      return { tab: 'grammar', grammarSlug: safeDecodeURIComponent(rawSlug) }
    }
    return { tab: 'grammar', grammarSlug: null }
  }

  if (normalized === '/conjugation') {
    return { tab: 'conjugation' }
  }

  if (normalized === '/reading') {
    return { tab: 'reading' }
  }

  if (normalized === '/wanikani') {
    return { tab: 'wanikani' }
  }

  return { tab: 'dashboard' }
}

export function buildRoutePath(route: AppRoute): string {
  switch (route.tab) {
    case 'dashboard':
      return '/'
    case 'reviews':
      if (route.reviewMode && route.reviewMode !== 'review') {
        return `/reviews?mode=${encodeURIComponent(route.reviewMode)}`
      }
      return '/reviews'
    case 'grammar':
      if (route.grammarSlug) {
        return `/grammar/${encodeURIComponent(route.grammarSlug)}`
      }
      return '/grammar'
    case 'conjugation':
      return '/conjugation'
    case 'reading':
      return '/reading'
    case 'wanikani':
      return '/wanikani'
    default:
      return '/'
  }
}

export function navigateTo(path: string, replace = false): void {
  if (typeof window === 'undefined' || !window.history) return
  const current = `${window.location.pathname}${window.location.search}`
  if (current === path) return
  if (replace) {
    window.history.replaceState(null, '', path)
  } else {
    window.history.pushState(null, '', path)
  }
}

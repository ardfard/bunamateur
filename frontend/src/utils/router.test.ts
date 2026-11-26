import { describe, expect, test } from 'bun:test'
import { buildRoutePath, parseRoute } from './router'

describe('parseRoute', () => {
  test('parses root and primary tabs', () => {
    expect(parseRoute('/')).toEqual({ tab: 'dashboard' })
    expect(parseRoute('/reviews')).toEqual({ tab: 'reviews' })
    expect(parseRoute('/grammar')).toEqual({ tab: 'grammar', grammarSlug: null })
    expect(parseRoute('/conjugation')).toEqual({ tab: 'conjugation' })
    expect(parseRoute('/reading')).toEqual({ tab: 'reading' })
    expect(parseRoute('/wanikani')).toEqual({ tab: 'wanikani' })
  })

  test('parses review modes from query string', () => {
    expect(parseRoute('/reviews', '?mode=review')).toEqual({ tab: 'reviews', reviewMode: 'review' })
    expect(parseRoute('/reviews', '?mode=lesson')).toEqual({ tab: 'reviews', reviewMode: 'lesson' })
    expect(parseRoute('/reviews', '?mode=cram')).toEqual({ tab: 'reviews', reviewMode: 'cram' })
    expect(parseRoute('/reviews', '?mode=ghost')).toEqual({ tab: 'reviews', reviewMode: 'ghost' })
    expect(parseRoute('/reviews', '?mode=unknown')).toEqual({ tab: 'reviews' })
  })

  test('parses grammar deep links with slug decoding and trailing slashes', () => {
    expect(parseRoute('/grammar/te-iru')).toEqual({ tab: 'grammar', grammarSlug: 'te-iru' })
    expect(parseRoute('/grammar/te-iru/')).toEqual({ tab: 'grammar', grammarSlug: 'te-iru' })
    expect(parseRoute('/grammar/%E3%81%AE%E3%81%AB')).toEqual({
      tab: 'grammar',
      grammarSlug: 'のに',
    })
  })

  test('falls back cleanly to dashboard on unknown routes', () => {
    expect(parseRoute('/not-a-real-page')).toEqual({ tab: 'dashboard' })
    expect(parseRoute('/foo/bar/baz')).toEqual({ tab: 'dashboard' })
  })
})

describe('buildRoutePath', () => {
  test('builds canonical paths for all routes', () => {
    expect(buildRoutePath({ tab: 'dashboard' })).toBe('/')
    expect(buildRoutePath({ tab: 'reviews' })).toBe('/reviews')
    expect(buildRoutePath({ tab: 'reviews', reviewMode: 'review' })).toBe('/reviews')
    expect(buildRoutePath({ tab: 'reviews', reviewMode: 'ghost' })).toBe('/reviews?mode=ghost')
    expect(buildRoutePath({ tab: 'reviews', reviewMode: 'cram' })).toBe('/reviews?mode=cram')
    expect(buildRoutePath({ tab: 'grammar', grammarSlug: null })).toBe('/grammar')
    expect(buildRoutePath({ tab: 'grammar', grammarSlug: 'te-iru' })).toBe('/grammar/te-iru')
    expect(buildRoutePath({ tab: 'conjugation' })).toBe('/conjugation')
    expect(buildRoutePath({ tab: 'reading' })).toBe('/reading')
    expect(buildRoutePath({ tab: 'wanikani' })).toBe('/wanikani')
  })
})

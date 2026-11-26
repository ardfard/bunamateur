import { describe, expect, test } from 'bun:test'
import { parseRubyMarkup, stripRuby } from './furigana'

describe('parseRubyMarkup', () => {
  test('returns plain segment when no ruby markup exists', () => {
    expect(parseRubyMarkup('これはテストです。')).toEqual([{ text: 'これはテストです。' }])
  })

  test('parses single ruby token surrounded by plain text', () => {
    expect(parseRubyMarkup('毎日[日本語|にほんご]を勉強します。')).toEqual([
      { text: '毎日' },
      { text: '日本語', reading: 'にほんご' },
      { text: 'を勉強します。' },
    ])
  })

  test('parses multiple consecutive and separated ruby tokens', () => {
    expect(parseRubyMarkup('[今日|きょう]は[天気|てんき]が[良|よ]い。')).toEqual([
      { text: '今日', reading: 'きょう' },
      { text: 'は' },
      { text: '天気', reading: 'てんき' },
      { text: 'が' },
      { text: '良', reading: 'よ' },
      { text: 'い。' },
    ])
  })

  test('handles malformed brackets safely without crashing or dropping text', () => {
    expect(parseRubyMarkup('[漢|かん')).toEqual([{ text: '[漢|かん' }])
    expect(parseRubyMarkup('[漢字]テスト')).toEqual([{ text: '[漢字]テスト' }])
    expect(parseRubyMarkup('前[壊れた]後[正|ただ]しい')).toEqual([
      { text: '前[壊れた]後' },
      { text: '正', reading: 'ただ' },
      { text: 'しい' },
    ])
  })

  test('handles empty input', () => {
    expect(parseRubyMarkup('')).toEqual([])
  })
})

describe('stripRuby', () => {
  test('strips all ruby readings and keeps base Japanese text', () => {
    expect(stripRuby('[私|わたし]は[日本語|にほんご]を[勉強|べんきょう]している。')).toBe(
      '私は日本語を勉強している。'
    )
  })

  test('preserves plain text and malformed brackets cleanly', () => {
    expect(stripRuby('そのままの文[不完全')).toBe('そのままの文[不完全')
  })
})

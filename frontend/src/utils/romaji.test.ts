import { describe, expect, test } from 'bun:test'
import { applyImeWithCursor, toHiraganaLive, toKanaLive, toKatakanaLive } from './romaji'

describe('romaji IME conversion', () => {
  test('converts standard Hepburn and Kunrei romaji to Hiragana', () => {
    expect(toHiraganaLive('noni')).toBe('のに')
    expect(toHiraganaLive('taberu')).toBe('たべる')
    expect(toHiraganaLive('shi')).toBe('し')
    expect(toHiraganaLive('si')).toBe('し')
    expect(toHiraganaLive('chi')).toBe('ち')
    expect(toHiraganaLive('ti')).toBe('ち')
    expect(toHiraganaLive('tsu')).toBe('つ')
    expect(toHiraganaLive('tu')).toBe('つ')
    expect(toHiraganaLive('fu')).toBe('ふ')
    expect(toHiraganaLive('hu')).toBe('ふ')
    expect(toHiraganaLive('sha')).toBe('しゃ')
    expect(toHiraganaLive('sya')).toBe('しゃ')
    expect(toHiraganaLive('cha')).toBe('ちゃ')
    expect(toHiraganaLive('tya')).toBe('ちゃ')
  })

  test('converts to Katakana with toKatakanaLive and toKanaLive', () => {
    expect(toKatakanaLive('ko-hi-')).toBe('コーヒー')
    expect(toKatakanaLive('pasokon', true)).toBe('パソコン')
    expect(toKanaLive('あkatakana', 'katakana')).toBe('あカタカナ')
  })

  test('leaves input unchanged in off mode', () => {
    expect(toKanaLive('konnichiwa', 'off', true)).toBe('konnichiwa')
  })

  test('handles sokuon double consonants and tch', () => {
    expect(toHiraganaLive('kitte')).toBe('きって')
    expect(toHiraganaLive('zasshi')).toBe('ざっし')
    expect(toHiraganaLive('matcha')).toBe('まっちゃ')
    expect(toKatakanaLive('kappu')).toBe('カップ')
    expect(toHiraganaLive('kk')).toBe('っk')
    expect(toKatakanaLive('kk')).toBe('ッk')
  })

  test('handles nn, n apostrophe, n before consonant, and finalize', () => {
    expect(toHiraganaLive('nn')).toBe('ん')
    expect(toHiraganaLive("n'")).toBe('ん')
    expect(toHiraganaLive('kantan')).toBe('かんたn')
    expect(toHiraganaLive('kantan', true)).toBe('かんたん')
    expect(toHiraganaLive('kanji')).toBe('かんじ')
    expect(toKatakanaLive('kantan', true)).toBe('カンタン')
  })

  test('handles small kana and extended foreign sound combinations', () => {
    expect(toHiraganaLive('ltu')).toBe('っ')
    expect(toHiraganaLive('xtu')).toBe('っ')
    expect(toHiraganaLive('la')).toBe('ぁ')
    expect(toHiraganaLive('xa')).toBe('ぁ')
    expect(toHiraganaLive('lya')).toBe('ゃ')
    expect(toHiraganaLive('xya')).toBe('ゃ')
    expect(toKatakanaLive('wi')).toBe('ウィ')
    expect(toKatakanaLive('we')).toBe('ウェ')
    expect(toKatakanaLive('va')).toBe('ヴァ')
    expect(toKatakanaLive('vi')).toBe('ヴィ')
    expect(toKatakanaLive('vu')).toBe('ヴ')
    expect(toKatakanaLive('ve')).toBe('ヴェ')
    expect(toKatakanaLive('vo')).toBe('ヴォ')
    expect(toKatakanaLive('tsa')).toBe('ツァ')
    expect(toKatakanaLive('tsi')).toBe('ツィ')
    expect(toKatakanaLive('tse')).toBe('ツェ')
    expect(toKatakanaLive('tso')).toBe('ツォ')
    expect(toKatakanaLive('fa')).toBe('ファ')
    expect(toKatakanaLive('fi')).toBe('フィ')
    expect(toKatakanaLive('fe')).toBe('フェ')
    expect(toKatakanaLive('fo')).toBe('フォ')
    expect(toHiraganaLive('dyi')).toBe('ぢぃ')
    expect(toKatakanaLive('thi')).toBe('ティ')
    expect(toKatakanaLive('dhi')).toBe('ディ')
    expect(toKatakanaLive('twu')).toBe('トゥ')
    expect(toKatakanaLive('dwu')).toBe('ドゥ')
  })

  test('preserves mid-string cursor position cleanly with applyImeWithCursor', () => {
    // Inserting 'ka' before 'たべる' at cursor index 2
    expect(applyImeWithCursor('kaたべる', 2, 'hiragana')).toEqual({
      value: 'かたべる',
      cursor: 1,
    })

    // Inserting 'shi' in the middle of 'あたべる' -> 'あshiたべる' with cursor after 'shi' (index 4)
    expect(applyImeWithCursor('あshiたべる', 4, 'hiragana')).toEqual({
      value: 'あしたべる',
      cursor: 2,
    })

    // Partial romaji before cursor stays intact and cursor stays right after it
    expect(applyImeWithCursor('あkたべる', 2, 'hiragana')).toEqual({
      value: 'あkたべる',
      cursor: 2,
    })

    // Katakana mid-string edit
    expect(applyImeWithCursor('あkaい', 3, 'katakana')).toEqual({
      value: 'あカい',
      cursor: 2,
    })

    // Off mode does not alter text or cursor
    expect(applyImeWithCursor('abたべる', 2, 'off')).toEqual({
      value: 'abたべる',
      cursor: 2,
    })
  })
})

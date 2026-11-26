// Real-time wanakana-compatible Romaji -> Hiragana / Katakana IME converter
// with mid-string cursor preservation.

export type ImeMode = 'hiragana' | 'katakana' | 'off'

const ROMAJI_TABLE: Record<string, string> = {
  // 4-char combinations
  ltsu: 'っ',
  xtsu: 'っ',

  // 3-char Hepburn, Kunrei, small kana, and extended combinations
  sha: 'しゃ',
  shi: 'し',
  shu: 'しゅ',
  she: 'しぇ',
  sho: 'しょ',
  cha: 'ちゃ',
  chi: 'ち',
  chu: 'ちゅ',
  che: 'ちぇ',
  cho: 'ちょ',
  tsu: 'つ',
  tsa: 'つぁ',
  tsi: 'つぃ',
  tse: 'つぇ',
  tso: 'つぉ',

  kya: 'きゃ',
  kyi: 'きぃ',
  kyu: 'きゅ',
  kye: 'きぇ',
  kyo: 'きょ',
  gya: 'ぎゃ',
  gyi: 'ぎぃ',
  gyu: 'ぎゅ',
  gye: 'ぎぇ',
  gyo: 'ぎょ',
  sya: 'しゃ',
  syi: 'しぃ',
  syu: 'しゅ',
  sye: 'しぇ',
  syo: 'しょ',
  zya: 'じゃ',
  zyi: 'じぃ',
  zyu: 'じゅ',
  zye: 'じぇ',
  zyo: 'じょ',
  jya: 'じゃ',
  jyi: 'じぃ',
  jyu: 'じゅ',
  jye: 'じぇ',
  jyo: 'じょ',
  tya: 'ちゃ',
  tyi: 'ちぃ',
  tyu: 'ちゅ',
  tye: 'ちぇ',
  tyo: 'ちょ',
  cya: 'ちゃ',
  cyi: 'ちぃ',
  cyu: 'ちゅ',
  cye: 'ちぇ',
  cyo: 'ちょ',
  dya: 'ぢゃ',
  dyi: 'ぢぃ',
  dyu: 'ぢゅ',
  dye: 'ぢぇ',
  dyo: 'ぢょ',
  tha: 'てゃ',
  thi: 'てぃ',
  thu: 'てゅ',
  the: 'てぇ',
  tho: 'てょ',
  dha: 'でゃ',
  dhi: 'でぃ',
  dhu: 'でゅ',
  dhe: 'でぇ',
  dho: 'でょ',
  twu: 'とぅ',
  dwu: 'どぅ',
  nya: 'にゃ',
  nyi: 'にぃ',
  nyu: 'にゅ',
  nye: 'にぇ',
  nyo: 'にょ',
  hya: 'ひゃ',
  hyi: 'ひぃ',
  hyu: 'ひゅ',
  hye: 'ひぇ',
  hyo: 'ひょ',
  bya: 'びゃ',
  byi: 'びぃ',
  byu: 'びゅ',
  bye: 'びぇ',
  byo: 'びょ',
  pya: 'ぴゃ',
  pyi: 'ぴぃ',
  pyu: 'ぴゅ',
  pye: 'ぴぇ',
  pyo: 'ぴょ',
  mya: 'みゃ',
  myi: 'みぃ',
  myu: 'みゅ',
  mye: 'みぇ',
  myo: 'みょ',
  rya: 'りゃ',
  ryi: 'りぃ',
  ryu: 'りゅ',
  rye: 'りぇ',
  ryo: 'りょ',
  fya: 'ふゃ',
  fyu: 'ふゅ',
  fyo: 'ふょ',
  vya: 'ゔゃ',
  vyu: 'ゔゅ',
  vyo: 'ゔょ',
  wha: 'うぁ',
  whi: 'うぃ',
  whu: 'う',
  whe: 'うぇ',
  who: 'うぉ',
  kwa: 'くぁ',
  gwa: 'ぐぁ',

  // Small kana (3-char)
  xya: 'ゃ',
  xyu: 'ゅ',
  xyo: 'ょ',
  lya: 'ゃ',
  lyu: 'ゅ',
  lyo: 'ょ',
  xtu: 'っ',
  ltu: 'っ',
  xwa: 'ゎ',
  lwa: 'ゎ',
  xka: 'ヵ',
  lka: 'ヵ',
  xke: 'ヶ',
  lke: 'ヶ',

  // 2-char combinations
  ka: 'か',
  ki: 'き',
  ku: 'く',
  ke: 'け',
  ko: 'こ',
  ca: 'か',
  ci: 'し',
  cu: 'く',
  ce: 'せ',
  co: 'こ',
  ga: 'が',
  gi: 'ぎ',
  gu: 'ぐ',
  ge: 'げ',
  go: 'ご',
  sa: 'さ',
  si: 'し',
  su: 'す',
  se: 'せ',
  so: 'そ',
  za: 'ざ',
  zi: 'じ',
  zu: 'ず',
  ze: 'ぜ',
  zo: 'ぞ',
  ja: 'じゃ',
  ji: 'じ',
  ju: 'じゅ',
  je: 'じぇ',
  jo: 'じょ',
  ta: 'た',
  ti: 'ち',
  tu: 'つ',
  te: 'て',
  to: 'と',
  da: 'だ',
  di: 'ぢ',
  du: 'づ',
  de: 'で',
  do: 'ど',
  na: 'な',
  ni: 'に',
  nu: 'ぬ',
  ne: 'ね',
  no: 'の',
  ha: 'は',
  hi: 'ひ',
  hu: 'ふ',
  fu: 'ふ',
  he: 'へ',
  ho: 'ほ',
  ba: 'ば',
  bi: 'び',
  bu: 'ぶ',
  be: 'べ',
  bo: 'ぼ',
  pa: 'ぱ',
  pi: 'ぴ',
  pu: 'ぷ',
  pe: 'ぺ',
  po: 'ぽ',
  ma: 'ま',
  mi: 'み',
  mu: 'む',
  me: 'め',
  mo: 'も',
  ya: 'や',
  yu: 'ゆ',
  yo: 'よ',
  ra: 'ら',
  ri: 'り',
  ru: 'る',
  re: 'れ',
  ro: 'ろ',
  wa: 'わ',
  wi: 'うぃ',
  we: 'うぇ',
  wo: 'を',
  fa: 'ふぁ',
  fi: 'ふぃ',
  fe: 'ふぇ',
  fo: 'ふぉ',
  va: 'ゔぁ',
  vi: 'ゔぃ',
  vu: 'ゔ',
  ve: 'ゔぇ',
  vo: 'ゔぉ',
  qa: 'くぁ',
  qi: 'くぃ',
  qu: 'く',
  qe: 'くぇ',
  qo: 'くぉ',
  nn: 'ん',
  "n'": 'ん',
  xa: 'ぁ',
  xi: 'ぃ',
  xu: 'ぅ',
  xe: 'ぇ',
  xo: 'ぉ',
  la: 'ぁ',
  li: 'ぃ',
  lu: 'ぅ',
  le: 'ぇ',
  lo: 'ぉ',

  // 1-char vowels & punctuation
  a: 'あ',
  i: 'い',
  u: 'う',
  e: 'え',
  o: 'お',
  '-': 'ー',
}

const VOWELS = new Set(['a', 'i', 'u', 'e', 'o', 'y'])
const SOKUON_CONSONANTS = new Set([
  'b', 'c', 'd', 'f', 'g', 'h', 'j', 'k', 'm', 'p', 'q', 'r', 's', 't', 'v', 'w', 'z',
])

function hiraganaToKatakana(text: string): string {
  let out = ''
  for (let i = 0; i < text.length; i++) {
    const code = text.charCodeAt(i)
    // Hiragana ぁ (0x3041) through ゔ (0x3094) -> Katakana ァ..ヴ (+0x60)
    // Note: ヵ (0x30F5) and ヶ (0x30F6) are already in Katakana block in ROMAJI_TABLE
    if (code >= 0x3041 && code <= 0x3094) {
      out += String.fromCharCode(code + 0x60)
    } else {
      out += text[i]
    }
  }
  return out
}

function formatKana(hiragana: string, mode: 'hiragana' | 'katakana'): string {
  return mode === 'katakana' ? hiraganaToKatakana(hiragana) : hiragana
}

export function toKanaLive(input: string, mode: ImeMode, finalize = false): string {
  if (mode === 'off') return input

  let out = ''
  let i = 0
  const s = input

  while (i < s.length) {
    const ch = s[i]
    const lower = ch.toLowerCase()

    // Non-ASCII (already kana/kanji) passes through directly
    if (ch.charCodeAt(0) > 127) {
      out += ch
      i += 1
      continue
    }

    // Handle Hepburn 'tch' -> っ + 'ch...' (e.g. matcha -> まっちゃ)
    if (
      lower === 't' &&
      i + 2 < s.length &&
      s.slice(i + 1, i + 3).toLowerCase() === 'ch'
    ) {
      out += formatKana('っ', mode)
      i += 1
      continue
    }

    // Handle double consonant -> っ / ッ (except n, l, x, y)
    if (
      i + 1 < s.length &&
      lower === s[i + 1].toLowerCase() &&
      SOKUON_CONSONANTS.has(lower)
    ) {
      out += formatKana('っ', mode)
      i += 1
      continue
    }

    // Check longest match (4, 3, 2, 1)
    let matched = false
    for (const len of [4, 3, 2, 1]) {
      if (i + len <= s.length) {
        const sub = s.slice(i, i + len).toLowerCase()
        if (ROMAJI_TABLE[sub]) {
          out += formatKana(ROMAJI_TABLE[sub], mode)
          i += len
          matched = true
          break
        }
      }
    }
    if (matched) continue

    // Handle standalone 'n' before non-vowel consonant (e.g. "kantan" -> かんたん)
    if (lower === 'n') {
      if (i + 1 < s.length) {
        const next = s[i + 1].toLowerCase()
        if (!VOWELS.has(next) && next !== 'n' && next !== "'") {
          out += formatKana('ん', mode)
          i += 1
          continue
        }
      } else if (finalize) {
        out += formatKana('ん', mode)
        i += 1
        continue
      }
    }

    // Preserve unfinished romaji buffer while typing
    out += ch
    i += 1
  }

  return out
}

export function toHiraganaLive(input: string, finalize = false): string {
  return toKanaLive(input, 'hiragana', finalize)
}

export function toKatakanaLive(input: string, finalize = false): string {
  return toKanaLive(input, 'katakana', finalize)
}

export function applyImeWithCursor(
  value: string,
  selectionStart: number,
  mode: ImeMode
): { value: string; cursor: number } {
  const clamped = Math.max(0, Math.min(selectionStart, value.length))
  if (mode === 'off') {
    return { value, cursor: clamped }
  }

  const prefix = value.slice(0, clamped)
  const suffix = value.slice(clamped)
  const convertedPrefix = toKanaLive(prefix, mode, false)

  return {
    value: convertedPrefix + suffix,
    cursor: convertedPrefix.length,
  }
}

// Real-time Romaji -> Hiragana IME converter for Japanese study inputs.

const ROMAJI_TABLE: Record<string, string> = {
  // 3-char & 4-char combinations
  ltsu: 'っ', xtsu: 'っ',
  sha: 'しゃ', shi: 'し', shu: 'しゅ', she: 'しぇ', sho: 'しょ',
  cha: 'ちゃ', chi: 'ち', chu: 'ちゅ', che: 'ちぇ', cho: 'ちょ',
  tsu: 'つ',
  kya: 'きゃ', kyu: 'きゅ', kyo: 'きょ',
  gya: 'ぎゃ', gyu: 'ぎゅ', gyo: 'ぎょ',
  sya: 'しゃ', syu: 'しゅ', syo: 'しょ',
  zya: 'じゃ', zyu: 'じゅ', zyo: 'じょ',
  jya: 'じゃ', jyu: 'じゅ', jyo: 'じょ',
  tya: 'ちゃ', tyu: 'ちゅ', tyo: 'ちょ',
  dya: 'ぢゃ', dyu: 'ぢゅ', dyo: 'ぢょ',
  nya: 'にゃ', nyu: 'にゅ', nyo: 'にょ',
  hya: 'ひゃ', hyu: 'ひゅ', hyo: 'ひょ',
  bya: 'びゃ', byu: 'びゅ', byo: 'びょ',
  pya: 'ぴゃ', pyu: 'ぴゅ', pyo: 'ぴょ',
  mya: 'みゃ', myu: 'みゅ', myo: 'みょ',
  rya: 'りゃ', ryu: 'りゅ', ryo: 'りょ',
  xya: 'ゃ', xyu: 'ゅ', xyo: 'ょ',
  lya: 'ゃ', lyu: 'ゅ', lyo: 'ょ',
  xtu: 'っ', ltu: 'っ',

  // 2-char combinations
  ka: 'か', ki: 'き', ku: 'く', ke: 'け', ko: 'こ',
  ga: 'が', gi: 'ぎ', gu: 'ぐ', ge: 'げ', go: 'ご',
  sa: 'さ', si: 'し', su: 'す', se: 'せ', so: 'そ',
  za: 'ざ', zi: 'じ', zu: 'ず', ze: 'ぜ', zo: 'ぞ',
  ja: 'じゃ', ji: 'じ', ju: 'じゅ', je: 'じぇ', jo: 'じょ',
  ta: 'た', ti: 'ち', tu: 'つ', te: 'て', to: 'と',
  da: 'だ', di: 'ぢ', du: 'づ', de: 'で', do: 'ど',
  na: 'な', ni: 'に', nu: 'ぬ', ne: 'ね', no: 'の',
  ha: 'は', hi: 'ひ', hu: 'ふ', fu: 'ふ', he: 'へ', ho: 'ほ',
  ba: 'ば', bi: 'び', bu: 'ぶ', be: 'べ', bo: 'ぼ',
  pa: 'ぱ', pi: 'ぴ', pu: 'ぷ', pe: 'ぺ', po: 'ぽ',
  ma: 'ま', mi: 'み', mu: 'む', me: 'め', mo: 'も',
  ya: 'や', yu: 'ゆ', yo: 'よ',
  ra: 'ら', ri: 'り', ru: 'る', re: 'れ', ro: 'ろ',
  wa: 'わ', wi: 'うぃ', we: 'うぇ', wo: 'を',
  fa: 'ふぁ', fi: 'ふぃ', fe: 'ふぇ', fo: 'ふぉ',
  nn: 'ん', "n'": 'ん',
  xa: 'ぁ', xi: 'ぃ', xu: 'ぅ', xe: 'ぇ', xo: 'ぉ',
  la: 'ぁ', li: 'ぃ', lu: 'ぅ', le: 'ぇ', lo: 'ぉ',

  // 1-char vowels & punctuation
  a: 'あ', i: 'い', u: 'う', e: 'え', o: 'お',
  '-': 'ー',
}

const VOWELS = new Set(['a', 'i', 'u', 'e', 'o', 'y'])
const CONSONANTS = new Set([
  'b', 'c', 'd', 'f', 'g', 'h', 'j', 'k', 'l', 'm', 'p', 'q', 'r', 's', 't', 'v', 'w', 'x', 'z',
])

export function toHiraganaLive(input: string, finalize = false): string {
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

    // Handle double consonant -> っ (except n)
    if (
      i + 1 < s.length &&
      lower === s[i + 1].toLowerCase() &&
      CONSONANTS.has(lower)
    ) {
      out += 'っ'
      i += 1
      continue
    }

    // Check longest match (4, 3, 2, 1)
    let matched = false
    for (const len of [4, 3, 2, 1]) {
      if (i + len <= s.length) {
        const sub = s.slice(i, i + len).toLowerCase()
        if (ROMAJI_TABLE[sub]) {
          out += ROMAJI_TABLE[sub]
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
          out += 'ん'
          i += 1
          continue
        }
      } else if (finalize) {
        out += 'ん'
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

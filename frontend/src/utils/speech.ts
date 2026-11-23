import { stripRuby } from './furigana'

export function speakJapanese(textWithOptionalRuby: string, rate = 0.95) {
  if (typeof window === 'undefined' || !('speechSynthesis' in window)) {
    return
  }
  const clean = stripRuby(textWithOptionalRuby)
  window.speechSynthesis.cancel()
  const utter = new SpeechSynthesisUtterance(clean)
  utter.lang = 'ja-JP'
  utter.rate = rate

  const voices = window.speechSynthesis.getVoices()
  const jpVoice = voices.find((v) => v.lang.toLowerCase().startsWith('ja'))
  if (jpVoice) {
    utter.voice = jpVoice
  }
  window.speechSynthesis.speak(utter)
}

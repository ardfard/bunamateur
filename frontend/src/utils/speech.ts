import { stripRuby } from './furigana'

let activeAudio: HTMLAudioElement | null = null
const sentenceAudioCache = new Map<number, 'missing' | string>()

export function stopJapaneseAudio() {
  if (activeAudio) {
    try {
      activeAudio.pause()
      activeAudio.currentTime = 0
    } catch {
      // ignore cleanup errors
    }
    activeAudio = null
  }
  if (typeof window !== 'undefined' && 'speechSynthesis' in window) {
    try {
      window.speechSynthesis.cancel()
    } catch {
      // ignore speech synthesis cancellation errors
    }
  }
}

export function speakJapanese(textWithOptionalRuby: string, rate = 0.95) {
  stopJapaneseAudio()
  if (typeof window === 'undefined' || !('speechSynthesis' in window)) {
    return
  }
  const clean = stripRuby(textWithOptionalRuby)
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

async function playAudioUrl(url: string, rate: number): Promise<void> {
  const audio = new Audio(url)
  audio.playbackRate = rate
  activeAudio = audio
  audio.addEventListener(
    'ended',
    () => {
      if (activeAudio === audio) {
        activeAudio = null
      }
    },
    { once: true }
  )
  await audio.play()
}

export async function playSentenceAudio(
  sentenceId: number,
  fallbackText: string,
  rate = 1.0
): Promise<'asset' | 'speech'> {
  stopJapaneseAudio()

  if (typeof window === 'undefined' || typeof Audio === 'undefined') {
    speakJapanese(fallbackText, rate)
    return 'speech'
  }

  const cached = sentenceAudioCache.get(sentenceId)
  if (cached === 'missing') {
    speakJapanese(fallbackText, rate)
    return 'speech'
  }

  if (cached) {
    try {
      await playAudioUrl(cached, rate)
      return 'asset'
    } catch {
      sentenceAudioCache.set(sentenceId, 'missing')
      speakJapanese(fallbackText, rate)
      return 'speech'
    }
  }

  try {
    const res = await fetch(`/api/audio/sentence/${sentenceId}`, {
      credentials: 'include',
    })
    if (!res.ok) {
      sentenceAudioCache.set(sentenceId, 'missing')
      speakJapanese(fallbackText, rate)
      return 'speech'
    }

    const blob = await res.blob()
    if (blob.size === 0) {
      sentenceAudioCache.set(sentenceId, 'missing')
      speakJapanese(fallbackText, rate)
      return 'speech'
    }

    const objectUrl = URL.createObjectURL(blob)
    sentenceAudioCache.set(sentenceId, objectUrl)
    await playAudioUrl(objectUrl, rate)
    return 'asset'
  } catch {
    sentenceAudioCache.set(sentenceId, 'missing')
    speakJapanese(fallbackText, rate)
    return 'speech'
  }
}

import React from 'react'
import type { FuriganaMode } from '../types'

interface RubySegment {
  text: string
  reading?: string
}

export function parseRubyMarkup(input: string): RubySegment[] {
  const segments: RubySegment[] = []
  let i = 0
  let plainBuf = ''

  while (i < input.length) {
    if (input[i] === '[') {
      const pipe = input.indexOf('|', i + 1)
      const close = input.indexOf(']', i + 1)
      if (pipe !== -1 && close !== -1 && pipe < close) {
        if (plainBuf) {
          segments.push({ text: plainBuf })
          plainBuf = ''
        }
        segments.push({
          text: input.slice(i + 1, pipe),
          reading: input.slice(pipe + 1, close),
        })
        i = close + 1
        continue
      }
    }
    plainBuf += input[i]
    i += 1
  }

  if (plainBuf) {
    segments.push({ text: plainBuf })
  }
  return segments
}

export function stripRuby(input: string): string {
  return parseRubyMarkup(input)
    .map((s) => s.text)
    .join('')
}

function isKanjiChar(ch: string): boolean {
  const code = ch.charCodeAt(0)
  return (code >= 0x4e00 && code <= 0x9fff) || (code >= 0x3400 && code <= 0x4dbf)
}

function isSegmentKnownInWK(text: string, knownKanji: Set<string>): boolean {
  const kanjiChars = [...text].filter(isKanjiChar)
  if (kanjiChars.length === 0) return false
  return kanjiChars.every((k) => knownKanji.has(k))
}

interface RubyTextProps {
  text: string
  furiganaMode: FuriganaMode
  knownKanji: Set<string>
  highlightKnown?: boolean
  className?: string
}

export const RubyText: React.FC<RubyTextProps> = ({
  text,
  furiganaMode,
  knownKanji,
  highlightKnown = false,
  className = '',
}) => {
  const segments = parseRubyMarkup(text)

  return (
    <span className={`font-jp ${className}`}>
      {segments.map((seg, idx) => {
        if (!seg.reading) {
          return <React.Fragment key={idx}>{seg.text}</React.Fragment>
        }
        const wkKnown = knownKanji.size > 0 && isSegmentKnownInWK(seg.text, knownKanji)
        const showReading =
          furiganaMode === 'all' || (furiganaMode === 'smart' && !wkKnown)

        return (
          <ruby
            key={idx}
            title={
              wkKnown
                ? `${seg.reading} • Known in WaniKani`
                : `${seg.reading}`
            }
            className={
              wkKnown && highlightKnown ? 'wk-known-kanji cursor-help' : 'cursor-help'
            }
          >
            {seg.text}
            <rp>(</rp>
            <rt style={{ visibility: showReading ? 'visible' : 'hidden' }}>
              {seg.reading}
            </rt>
            <rp>)</rp>
          </ruby>
        )
      })}
    </span>
  )
}

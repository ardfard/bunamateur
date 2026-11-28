import React from 'react'
import type { FuriganaMode, GrammarLesson, LessonCheck } from '../types'
import { RubyText } from '../utils/furigana'

export type LessonContentMode = 'full' | 'read' | 'examples' | 'checks'

interface LessonContentProps {
  lesson: GrammarLesson
  furiganaMode: FuriganaMode
  knownKanji: Set<string>
  highlightKnown: boolean
  /** Which sections to render. Defaults to full lesson. */
  mode?: LessonContentMode
  /** Cap worked examples (examples mode / full). */
  exampleLimit?: number
  /** Cap quick checks (checks mode / full). */
  checkLimit?: number
  /**
   * Interactive check picks keyed by check index.
   * When provided with mode `checks` or `full`, options become clickable.
   */
  checkPicks?: Record<number, number>
  onCheckPick?: (checkIndex: number, optionIndex: number) => void
}

const SectionLabel: React.FC<{ children: React.ReactNode }> = ({ children }) => (
  <div
    className="font-mono-code mb-2 text-[11px] font-bold uppercase tracking-wider"
    style={{ color: 'var(--text-muted)' }}
  >
    {children}
  </div>
)

function Ruby({
  text,
  furiganaMode,
  knownKanji,
  highlightKnown,
  className = '',
}: {
  text: string
  furiganaMode: FuriganaMode
  knownKanji: Set<string>
  highlightKnown: boolean
  className?: string
}) {
  return (
    <RubyText
      text={text}
      furiganaMode={furiganaMode}
      knownKanji={knownKanji}
      highlightKnown={highlightKnown}
      className={className}
    />
  )
}

function CheckBlock({
  checks,
  furiganaMode,
  knownKanji,
  highlightKnown,
  checkPicks,
  onCheckPick,
}: {
  checks: LessonCheck[]
  furiganaMode: FuriganaMode
  knownKanji: Set<string>
  highlightKnown: boolean
  checkPicks?: Record<number, number>
  onCheckPick?: (checkIndex: number, optionIndex: number) => void
}) {
  const interactive = Boolean(onCheckPick)

  return (
    <div className="flex flex-col gap-4">
      <SectionLabel>Quick checks</SectionLabel>
      {checks.map((check, i) => {
        const picked = checkPicks?.[i]
        const answered = typeof picked === 'number'
        return (
          <div key={i} className="flex flex-col gap-2.5">
            <p className="font-jp text-base font-semibold leading-relaxed">
              <Ruby
                text={check.prompt}
                furiganaMode={furiganaMode}
                knownKanji={knownKanji}
                highlightKnown={highlightKnown}
              />
            </p>
            <div className="flex flex-wrap gap-2">
              {check.options.map((opt, oi) => {
                const isPicked = picked === oi
                const isCorrect = oi === check.answer_idx
                let bg = 'var(--bg-subtle)'
                let color = 'var(--text-primary)'
                let border = 'var(--border-subtle)'
                if (answered) {
                  if (isCorrect) {
                    bg = 'var(--accent-matcha-soft)'
                    color = 'var(--accent-matcha)'
                    border = 'var(--accent-matcha)'
                  } else if (isPicked) {
                    bg = 'var(--accent-vermilion-soft)'
                    color = 'var(--accent-vermilion)'
                    border = 'var(--accent-vermilion)'
                  }
                } else if (interactive) {
                  bg = 'var(--bg-canvas)'
                }
                return (
                  <button
                    key={oi}
                    type="button"
                    disabled={!interactive || answered}
                    onClick={() => onCheckPick?.(i, oi)}
                    className={`font-jp rounded-lg border px-3.5 py-2 text-sm font-medium ${
                      interactive && !answered ? 'cursor-pointer hover:opacity-90' : 'cursor-default'
                    }`}
                    style={{ backgroundColor: bg, color, borderColor: border }}
                  >
                    {opt}
                  </button>
                )
              })}
            </div>
            {answered && check.why && (
              <p className="text-sm leading-relaxed" style={{ color: 'var(--text-secondary)' }}>
                {check.why}
              </p>
            )}
          </div>
        )
      })}
    </div>
  )
}

export const LessonContent: React.FC<LessonContentProps> = ({
  lesson,
  furiganaMode,
  knownKanji,
  highlightKnown,
  mode = 'full',
  exampleLimit = 4,
  checkLimit = 3,
  checkPicks,
  onCheckPick,
}) => {
  const showRead = mode === 'full' || mode === 'read'
  const showExamples = mode === 'full' || mode === 'examples'
  const showChecks = mode === 'full' || mode === 'checks'

  const examples = lesson.examples.slice(0, exampleLimit)
  const checks = lesson.checks.slice(0, checkLimit)

  const rubyProps = { furiganaMode, knownKanji, highlightKnown }

  return (
    <div className="flex flex-col gap-6 text-left">
      {showRead && (
        <>
          {/* Gist */}
          <section>
            <SectionLabel>Gist</SectionLabel>
            <p className="text-sm leading-relaxed sm:text-base" style={{ color: 'var(--text-secondary)' }}>
              <Ruby text={lesson.gist} {...rubyProps} />
            </p>
          </section>

          {/* Formation table */}
          <section>
            <SectionLabel>Formation</SectionLabel>
            <div className="-mx-1 overflow-x-auto px-1">
              <table className="w-full min-w-[520px] border-collapse text-left text-sm">
                <thead>
                  <tr>
                    {lesson.formation.columns.map((col) => (
                      <th
                        key={col}
                        className="font-mono-code border-b px-2.5 py-2 text-[11px] font-bold uppercase tracking-wider whitespace-nowrap"
                        style={{
                          borderColor: 'var(--border-subtle)',
                          color: 'var(--text-muted)',
                        }}
                      >
                        {col}
                      </th>
                    ))}
                  </tr>
                </thead>
                <tbody>
                  {lesson.formation.rows.map((row, ri) => (
                    <tr key={ri}>
                      {row.map((cell, ci) => (
                        <td
                          key={ci}
                          className="font-jp border-b px-2.5 py-2.5 align-top leading-relaxed whitespace-nowrap"
                          style={{
                            borderColor: 'var(--border-subtle)',
                            color: 'var(--text-primary)',
                          }}
                        >
                          <Ruby text={cell} {...rubyProps} />
                        </td>
                      ))}
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
            {lesson.formation.note && (
              <p className="mt-2 text-xs leading-relaxed" style={{ color: 'var(--text-muted)' }}>
                <Ruby text={lesson.formation.note} {...rubyProps} />
              </p>
            )}
          </section>

          {/* Explanation */}
          <section>
            <SectionLabel>Explanation</SectionLabel>
            <p className="text-sm leading-relaxed whitespace-pre-line" style={{ color: 'var(--text-secondary)' }}>
              <Ruby text={lesson.explanation} {...rubyProps} />
            </p>
          </section>

          {/* Senses */}
          {lesson.senses.length > 0 && (
            <section className="flex flex-col gap-3">
              <SectionLabel>Senses</SectionLabel>
              {lesson.senses.map((sense, i) => (
                <div key={i} className="flex flex-col gap-1">
                  <div className="text-sm font-semibold" style={{ color: 'var(--accent-indigo)' }}>
                    {sense.label}
                  </div>
                  {sense.body && (
                    <p className="text-sm leading-relaxed" style={{ color: 'var(--text-secondary)' }}>
                      <Ruby text={sense.body} {...rubyProps} />
                    </p>
                  )}
                  <div className="font-jp text-base leading-loose">
                    <Ruby text={sense.example_ruby} {...rubyProps} />
                  </div>
                  <p className="text-sm" style={{ color: 'var(--text-muted)' }}>
                    {sense.example_en}
                  </p>
                </div>
              ))}
            </section>
          )}

          {/* Register */}
          {lesson.register_context && (
            <section>
              <SectionLabel>Register & context</SectionLabel>
              <p className="text-sm leading-relaxed" style={{ color: 'var(--text-secondary)' }}>
                <Ruby text={lesson.register_context} {...rubyProps} />
              </p>
            </section>
          )}

          {/* Mistakes */}
          {lesson.mistakes.length > 0 && (
            <section className="flex flex-col gap-3">
              <SectionLabel>Common mistakes</SectionLabel>
              {lesson.mistakes.map((m, i) => (
                <div key={i} className="flex flex-col gap-1.5">
                  <div className="flex flex-wrap items-start gap-x-3 gap-y-1 text-base leading-loose">
                    <span className="font-jp" style={{ color: 'var(--accent-vermilion)' }}>
                      ✗ <Ruby text={m.wrong_ruby} {...rubyProps} />
                    </span>
                    <span className="font-jp" style={{ color: 'var(--accent-matcha)' }}>
                      ✓ <Ruby text={m.right_ruby} {...rubyProps} />
                    </span>
                  </div>
                  <p className="text-sm leading-relaxed" style={{ color: 'var(--text-secondary)' }}>
                    {m.why}
                  </p>
                </div>
              ))}
            </section>
          )}

          {/* Contrasts */}
          {lesson.contrasts.length > 0 && (
            <section className="flex flex-col gap-4">
              <SectionLabel>Contrasts</SectionLabel>
              {lesson.contrasts.map((c, i) => (
                <div key={i} className="flex flex-col gap-2">
                  <div className="text-sm font-semibold" style={{ color: 'var(--text-primary)' }}>
                    {c.label}
                  </div>
                  <p className="text-sm leading-relaxed" style={{ color: 'var(--text-secondary)' }}>
                    {c.summary}
                  </p>
                  <div className="grid gap-3 sm:grid-cols-2">
                    <div>
                      <div className="font-jp text-base leading-loose">
                        <Ruby text={c.ex_a_ruby} {...rubyProps} />
                      </div>
                      <p className="mt-0.5 text-xs" style={{ color: 'var(--text-muted)' }}>
                        {c.ex_a_en}
                      </p>
                    </div>
                    <div>
                      <div className="font-jp text-base leading-loose">
                        <Ruby text={c.ex_b_ruby} {...rubyProps} />
                      </div>
                      <p className="mt-0.5 text-xs" style={{ color: 'var(--text-muted)' }}>
                        {c.ex_b_en}
                      </p>
                    </div>
                  </div>
                </div>
              ))}
            </section>
          )}

          {/* Related */}
          {(lesson.related.prerequisites?.length || lesson.related.next?.length) && (
            <section>
              <SectionLabel>Related</SectionLabel>
              <div className="flex flex-col gap-1 text-sm" style={{ color: 'var(--text-secondary)' }}>
                {lesson.related.prerequisites && lesson.related.prerequisites.length > 0 && (
                  <p>
                    <span className="font-medium" style={{ color: 'var(--text-primary)' }}>
                      Prerequisites:{' '}
                    </span>
                    {lesson.related.prerequisites.join(', ')}
                  </p>
                )}
                {lesson.related.next && lesson.related.next.length > 0 && (
                  <p>
                    <span className="font-medium" style={{ color: 'var(--text-primary)' }}>
                      Next:{' '}
                    </span>
                    {lesson.related.next.join(', ')}
                  </p>
                )}
              </div>
            </section>
          )}
        </>
      )}

      {showExamples && (
        <>
          {examples.length > 0 && (
            <section className="flex flex-col gap-3">
              <SectionLabel>Worked examples</SectionLabel>
              {examples.map((ex, i) => (
                <div key={i} className="flex flex-col gap-1">
                  <div className="font-jp text-lg leading-loose">
                    <Ruby text={ex.ruby} {...rubyProps} />
                  </div>
                  <p className="text-sm" style={{ color: 'var(--text-secondary)' }}>
                    {ex.english}
                  </p>
                  {ex.note && (
                    <p className="text-xs" style={{ color: 'var(--text-muted)' }}>
                      {ex.note}
                    </p>
                  )}
                </div>
              ))}
            </section>
          )}

          {lesson.dialogue.length > 0 && (
            <section className="flex flex-col gap-3">
              <SectionLabel>Dialogue</SectionLabel>
              {lesson.dialogue.map((line, i) => (
                <div key={i} className="flex gap-3">
                  <span
                    className="font-mono-code shrink-0 pt-1 text-xs font-bold"
                    style={{ color: 'var(--accent-indigo)' }}
                  >
                    {line.speaker}
                  </span>
                  <div className="flex min-w-0 flex-col gap-0.5">
                    <div className="font-jp text-base leading-loose">
                      <Ruby text={line.ruby} {...rubyProps} />
                    </div>
                    <p className="text-sm" style={{ color: 'var(--text-muted)' }}>
                      {line.english}
                    </p>
                  </div>
                </div>
              ))}
            </section>
          )}
        </>
      )}

      {showChecks && checks.length > 0 && (
        <CheckBlock
          checks={checks}
          {...rubyProps}
          checkPicks={checkPicks}
          onCheckPick={onCheckPick}
        />
      )}
    </div>
  )
}

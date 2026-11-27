import type { PlacementQuestion } from '../types'

export interface StudyPathOption {
  id: string
  label: string
  desc: string
  badge: string
}

export const STUDY_PATHS: readonly StudyPathOption[] = [
  {
    id: 'JLPT Core',
    label: 'JLPT Core',
    desc: 'Balanced grammar progression across all JLPT levels (N5 → N1)',
    badge: 'N5–N1',
  },
  {
    id: 'Genki I',
    label: 'Genki I',
    desc: 'Beginner essentials aligned with Genki Volume 1',
    badge: 'JLPT N5',
  },
  {
    id: 'Genki II',
    label: 'Genki II',
    desc: 'Elementary structures aligned with Genki Volume 2',
    badge: 'JLPT N4',
  },
  {
    id: 'Tobira',
    label: 'Tobira',
    desc: 'Intermediate bridge to authentic Japanese texts',
    badge: 'JLPT N3',
  },
  {
    id: 'Quartet',
    label: 'Quartet',
    desc: 'Upper-intermediate reading, writing, and nuance',
    badge: 'JLPT N2',
  },
] as const

export const DAILY_GOAL_OPTIONS: readonly number[] = [10, 15, 20, 30, 50] as const

export const JLPT_LEVEL_ORDER = ['N5', 'N4', 'N3', 'N2', 'N1'] as const

const PLACEMENT_RANK_ORDER = ['N1', 'N2', 'N3', 'N4'] as const

/**
 * Infers a learner's starting JLPT level from checked placement grammar items.
 * Checks highest-to-lowest (N1 -> N4) and defaults to N5 when none are matched.
 */
export function inferPlacementJlpt(
  questions: PlacementQuestion[],
  knownIds: Set<number>
): string {
  if (knownIds.size === 0 || questions.length === 0) {
    return 'N5'
  }

  const levels = new Set<string>()
  for (const question of questions) {
    if (knownIds.has(question.grammar_id)) {
      levels.add(question.jlpt_level)
    }
  }

  for (const level of PLACEMENT_RANK_ORDER) {
    if (levels.has(level)) {
      return level
    }
  }

  return 'N5'
}

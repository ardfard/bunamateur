import { describe, expect, test } from 'bun:test'
import type { PlacementQuestion } from '../types'
import { DAILY_GOAL_OPTIONS, STUDY_PATHS, inferPlacementJlpt } from './onboarding'

const SAMPLE_QUESTIONS: PlacementQuestion[] = [
  {
    grammar_id: 1,
    slug: 'desu',
    title: 'です',
    meaning: 'to be (polite)',
    jlpt_level: 'N5',
    study_path: 'Genki I',
    sample_jp: '私は学生です。',
    sample_en: 'I am a student.',
  },
  {
    grammar_id: 2,
    slug: 'te-iru',
    title: 'ている',
    meaning: 'ongoing state / action',
    jlpt_level: 'N5',
    study_path: 'Genki I',
    sample_jp: '今勉強しています。',
    sample_en: 'I am studying now.',
  },
  {
    grammar_id: 3,
    slug: 'ba-form',
    title: '〜ば',
    meaning: 'if / conditional',
    jlpt_level: 'N4',
    study_path: 'Genki II',
    sample_jp: '時間があれば行きます。',
    sample_en: 'If I have time, I will go.',
  },
  {
    grammar_id: 4,
    slug: 'wake-da',
    title: 'わけだ',
    meaning: 'it means that / no wonder',
    jlpt_level: 'N3',
    study_path: 'Tobira',
    sample_jp: 'それで遅れたわけだ。',
    sample_en: 'So that is why you were late.',
  },
  {
    grammar_id: 5,
    slug: 'ni-shitagatte',
    title: 'にしたがって',
    meaning: 'as / in accordance with',
    jlpt_level: 'N2',
    study_path: 'Quartet',
    sample_jp: '年をとるにしたがって体力が落ちる。',
    sample_en: 'As you get older, stamina declines.',
  },
  {
    grammar_id: 6,
    slug: 'o-yo-gi-naku-sareru',
    title: 'を余儀なくされる',
    meaning: 'to be forced to / have no choice but to',
    jlpt_level: 'N1',
    study_path: 'JLPT Core',
    sample_jp: '計画の変更を余儀なくされた。',
    sample_en: 'We were forced to change the plan.',
  },
]

describe('onboarding constants', () => {
  test('exposes expected study paths and daily goals', () => {
    expect(STUDY_PATHS.map((p) => p.id)).toEqual([
      'JLPT Core',
      'Genki I',
      'Genki II',
      'Tobira',
      'Quartet',
    ])
    expect(Array.from(DAILY_GOAL_OPTIONS)).toEqual([10, 15, 20, 30, 50])
  })
})

describe('inferPlacementJlpt', () => {
  test('defaults to N5 when selection is empty', () => {
    expect(inferPlacementJlpt(SAMPLE_QUESTIONS, new Set())).toBe('N5')
  })

  test('returns N5 when only N5 items are selected', () => {
    expect(inferPlacementJlpt(SAMPLE_QUESTIONS, new Set([1, 2]))).toBe('N5')
  })

  test('returns N4 when N4 is the highest known level', () => {
    expect(inferPlacementJlpt(SAMPLE_QUESTIONS, new Set([1, 3]))).toBe('N4')
  })

  test('returns N3 when N3 is the highest known level', () => {
    expect(inferPlacementJlpt(SAMPLE_QUESTIONS, new Set([1, 3, 4]))).toBe('N3')
  })

  test('returns N2 when N2 is the highest known level', () => {
    expect(inferPlacementJlpt(SAMPLE_QUESTIONS, new Set([2, 5]))).toBe('N2')
  })

  test('returns N1 when N1 is selected even alongside lower levels', () => {
    expect(inferPlacementJlpt(SAMPLE_QUESTIONS, new Set([1, 4, 6]))).toBe('N1')
    expect(inferPlacementJlpt(SAMPLE_QUESTIONS, new Set([6]))).toBe('N1')
  })

  test('ignores unknown grammar IDs and falls back safely', () => {
    expect(inferPlacementJlpt(SAMPLE_QUESTIONS, new Set([999, 1000]))).toBe('N5')
    expect(inferPlacementJlpt(SAMPLE_QUESTIONS, new Set([999, 4]))).toBe('N3')
    expect(inferPlacementJlpt([], new Set([1, 6]))).toBe('N5')
  })
})

import { describe, expect, test } from 'bun:test'
import { initialLessonStep, lessonStepLabel, nextLessonStep } from './lessonSteps'

describe('lessonSteps', () => {
  test('initialLessonStep starts at read when lesson exists', () => {
    expect(initialLessonStep(true)).toBe('read')
    expect(initialLessonStep(false)).toBe('cloze')
  })

  test('nextLessonStep advances read → examples → checks → cloze', () => {
    expect(nextLessonStep('read')).toBe('examples')
    expect(nextLessonStep('examples')).toBe('checks')
    expect(nextLessonStep('checks')).toBe('cloze')
    expect(nextLessonStep('cloze')).toBeNull()
  })

  test('lessonStepLabel covers each step', () => {
    expect(lessonStepLabel('read')).toBe('Read')
    expect(lessonStepLabel('examples')).toBe('Examples')
    expect(lessonStepLabel('checks')).toBe('Checks')
    expect(lessonStepLabel('cloze')).toBe('Practice')
  })
})

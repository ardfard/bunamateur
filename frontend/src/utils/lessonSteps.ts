/** Learn New multi-step flow when a rich `lesson` payload is present. */
export type LessonStep = 'read' | 'examples' | 'checks' | 'cloze'

export function initialLessonStep(hasLesson: boolean): LessonStep {
  return hasLesson ? 'read' : 'cloze'
}

export function nextLessonStep(step: LessonStep): LessonStep | null {
  switch (step) {
    case 'read':
      return 'examples'
    case 'examples':
      return 'checks'
    case 'checks':
      return 'cloze'
    case 'cloze':
      return null
  }
}

export function lessonStepLabel(step: LessonStep): string {
  switch (step) {
    case 'read':
      return 'Read'
    case 'examples':
      return 'Examples'
    case 'checks':
      return 'Checks'
    case 'cloze':
      return 'Practice'
  }
}

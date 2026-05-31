import type { BadgeProps } from '@/components/ui/badge'

/** Maps a record kind to its Badge variant. Subtasks share the task accent. */
export const KIND_VARIANT: Record<string, BadgeProps['variant']> = {
  epic: 'epic',
  task: 'task',
  subtask: 'task',
  bug: 'bug',
  feature: 'feature',
}

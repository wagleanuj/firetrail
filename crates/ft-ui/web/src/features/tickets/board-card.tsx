/**
 * Presentational body for a Kanban board card.
 *
 * The dnd draggable wrapper lives in board.tsx; this component is
 * purely concerned with rendering the card contents.
 */
import { Link } from '@tanstack/react-router'
import { Badge, type BadgeProps } from '@/components/ui/badge'
import { epicColor } from './epic-color'
import { PriorityBadge } from './board'
import type { BoardCard } from '@/api/types/BoardCard'

const KIND_VARIANT: Record<string, BadgeProps['variant']> = {
  epic: 'epic',
  task: 'task',
  subtask: 'task',
  bug: 'bug',
  feature: 'feature',
}

export function BoardCardBody({
  card,
  epicTitle,
}: {
  card: BoardCard
  epicTitle?: string
}) {
  const variant = KIND_VARIANT[card.kind] ?? 'secondary'
  const pct =
    card.criteria_total > 0
      ? Math.round((card.criteria_met / card.criteria_total) * 100)
      : 0

  return (
    <div className="flex flex-col gap-2.5">
      {/* Top row: kind pill + epic chip + priority */}
      <div className="flex items-center justify-between gap-2">
        <div className="flex min-w-0 items-center gap-2">
          <Badge
            variant={variant}
            className="px-1.5 py-0 text-[0.625rem] capitalize"
          >
            {card.kind}
          </Badge>
          {card.epic_id && epicTitle && (
            <span
              className="truncate rounded-full px-1.5 py-0.5 text-[0.625rem]"
              style={{
                background: `${epicColor(card.epic_id)}22`,
                color: epicColor(card.epic_id),
              }}
            >
              {epicTitle}
            </span>
          )}
        </div>
        <PriorityBadge priority={card.priority} />
      </div>

      {/* Title link */}
      <Link
        to="/tickets/$id"
        params={{ id: card.id }}
        className="block text-sm font-medium leading-snug text-foreground hover:text-primary"
        onPointerDown={(e) => e.stopPropagation()}
      >
        {card.title}
      </Link>

      {/* Criteria progress bar */}
      {card.criteria_total > 0 && (
        <div className="flex items-center gap-2">
          <div className="h-1 flex-1 overflow-hidden rounded-full bg-muted">
            <div
              className="h-full rounded-full bg-type-task"
              style={{ width: `${pct}%` }}
            />
          </div>
          <span className="font-mono text-[0.625rem] text-muted-foreground">
            {card.criteria_met}/{card.criteria_total}
          </span>
        </div>
      )}

      {/* Footer: short id, subtask count, blocked badge, owner */}
      <div className="flex items-center gap-2 text-[0.625rem] text-muted-foreground">
        <span className="font-mono">{card.short_id}</span>
        {card.subtask_count > 0 && <span>⛬ {card.subtask_count}</span>}
        {card.blocked_by_count > 0 && (
          <span className="rounded-full bg-destructive/15 px-1.5 py-0.5 text-destructive">
            ⊘ blocked
          </span>
        )}
        {card.owner && (
          <span className="ml-auto truncate">{card.owner}</span>
        )}
      </div>
    </div>
  )
}

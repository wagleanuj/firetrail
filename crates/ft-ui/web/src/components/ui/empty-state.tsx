/**
 * Reusable empty-state component.
 *
 * Renders an icon, title, optional description, and an optional call-to-action
 * inside a dashed-border card. Use this whenever a list, dashboard, or
 * collection has no rows to show — it provides consistent visual treatment
 * and contextual guidance across the app.
 */
import type { LucideIcon } from 'lucide-react'
import { cn } from '@/lib/utils'

export interface EmptyStateProps {
  /** Icon component (lucide-react) shown above the title. */
  icon: LucideIcon
  /** Short headline. */
  title: string
  /** Optional supporting text. */
  description?: string
  /** Optional call-to-action (e.g. a `<Button>`). */
  action?: React.ReactNode
  /** Optional class override on the outer card. */
  className?: string
}

export function EmptyState({
  icon: Icon,
  title,
  description,
  action,
  className,
}: EmptyStateProps) {
  return (
    <div
      data-testid="empty-state"
      className={cn(
        'mx-auto flex max-w-md flex-col items-center gap-3 rounded-xl border border-dashed border-border/70 bg-card/40 px-6 py-10 text-center',
        className,
      )}
    >
      <div className="flex h-12 w-12 items-center justify-center rounded-full bg-primary/10 text-primary">
        <Icon className="h-6 w-6" />
      </div>
      <h2 className="font-mono text-base font-semibold">{title}</h2>
      {description && (
        <p className="text-sm text-muted-foreground">{description}</p>
      )}
      {action && <div className="pt-1">{action}</div>}
    </div>
  )
}

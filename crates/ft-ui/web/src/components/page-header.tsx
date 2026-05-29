/**
 * Shared page header — the cohesion primitive every domain route renders
 * through. Provides a consistent title rhythm (Sora display face), an
 * optional subtitle, a right-aligned actions slot, and an optional tabs row
 * beneath the title.
 *
 * This component is intentionally minimal and layout-only; domains compose
 * their own action buttons / tab triggers and pass them in. See §6 of the
 * ft-ui redesign design spec.
 */
import * as React from 'react'
import { cn } from '@/lib/utils'

export interface PageHeaderProps {
  /** Page title. Rendered with the display face. */
  title: string
  /** Optional supporting line beneath the title (string or rich node). */
  subtitle?: React.ReactNode
  /** Right-aligned slot for primary/secondary actions. */
  actions?: React.ReactNode
  /** Optional row rendered under the title block (e.g. tab triggers). */
  tabs?: React.ReactNode
  /** Extra classes for the outer wrapper. */
  className?: string
}

export function PageHeader({ title, subtitle, actions, tabs, className }: PageHeaderProps) {
  return (
    <header className={cn('flex flex-col gap-3', className)}>
      <div className="flex items-start justify-between gap-4">
        <div className="min-w-0 space-y-1">
          <h1 className="truncate font-display text-xl font-semibold leading-snug tracking-tight">
            {title}
          </h1>
          {subtitle != null ? (
            <div className="text-sm text-muted-foreground">{subtitle}</div>
          ) : null}
        </div>
        {actions != null ? (
          <div className="flex shrink-0 items-center gap-2">{actions}</div>
        ) : null}
      </div>
      {tabs != null ? <div className="flex items-center">{tabs}</div> : null}
    </header>
  )
}

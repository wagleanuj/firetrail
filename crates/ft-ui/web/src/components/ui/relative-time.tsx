/**
 * `<RelativeTime>` renders a human "5 minutes ago"-style timestamp with the
 * absolute ISO string in a tooltip. Re-renders every 60s while mounted so the
 * relative label stays fresh without forcing parent renders.
 */
import * as React from 'react'
import { formatDistanceToNow, parseISO } from 'date-fns'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip'
import { cn } from '@/lib/utils'

interface RelativeTimeProps {
  value: string | Date | null | undefined
  className?: string
  /** Optional prefix like "created" / "updated". Renders before the relative text. */
  prefix?: string
}

export function RelativeTime({ value, className, prefix }: RelativeTimeProps) {
  // Force a re-render once a minute so the "ago" label stays current.
  const [, force] = React.useReducer((n: number) => n + 1, 0)
  React.useEffect(() => {
    if (!value) return
    const handle = window.setInterval(() => force(), 60_000)
    return () => window.clearInterval(handle)
  }, [value])

  if (!value) return <span className={className}>—</span>

  const date = typeof value === 'string' ? safeParse(value) : value
  if (!date) {
    return (
      <span className={className} title={String(value)}>
        {String(value)}
      </span>
    )
  }

  const relative = formatDistanceToNow(date, { addSuffix: true })
  const absolute = date.toLocaleString()

  return (
    <TooltipProvider delayDuration={150}>
      <Tooltip>
        <TooltipTrigger asChild>
          <time dateTime={date.toISOString()} className={cn('cursor-default', className)}>
            {prefix ? `${prefix} ${relative}` : relative}
          </time>
        </TooltipTrigger>
        <TooltipContent>
          <span className="font-mono text-xs">{absolute}</span>
        </TooltipContent>
      </Tooltip>
    </TooltipProvider>
  )
}

function safeParse(s: string): Date | null {
  try {
    const d = parseISO(s)
    if (Number.isFinite(d.getTime())) return d
  } catch {
    /* fallthrough */
  }
  const d = new Date(s)
  return Number.isFinite(d.getTime()) ? d : null
}

/**
 * Epics roll-up view with child progress bars and ready-to-close nudge.
 */
import * as React from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { Link } from '@tanstack/react-router'
import { Layers } from 'lucide-react'
import { PageHeader } from '@/components/page-header'
import { Button } from '@/components/ui/button'
import { Skeleton } from '@/components/ui/skeleton'
import { EmptyState } from '@/components/ui/empty-state'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import type { EpicSummary } from '@/api/types/EpicSummary'
import type { BoardCard } from '@/api/types/BoardCard'
import { BoardCardBody } from '@/features/tickets/board-card'
import { closeTicket } from '@/features/tickets/api'
import { toastApiError } from '@/api/error'
import { useEpicsQuery } from './use-epics-query'

// ---------------------------------------------------------------------------
// EpicRow
// ---------------------------------------------------------------------------

function EpicRow({
  epic,
  children,
  onCloseEpic,
}: {
  epic: EpicSummary
  children: BoardCard[]
  onCloseEpic: (id: string) => void
}) {
  const [expanded, setExpanded] = React.useState(false)

  const childPct =
    epic.child_total > 0
      ? Math.round((epic.child_closed / epic.child_total) * 100)
      : 0

  const criteriaPct =
    epic.criteria_total > 0
      ? Math.round((epic.criteria_met / epic.criteria_total) * 100)
      : 0

  return (
    <div className="rounded-lg border border-border/60 bg-card/40 p-4">
      {/* Header row */}
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <span className="font-mono text-xs text-muted-foreground">{epic.short_id}</span>
            <span className="rounded-full bg-muted px-1.5 py-0.5 text-[0.625rem] capitalize text-muted-foreground">
              {epic.status}
            </span>
            <span className="rounded-full bg-muted px-1.5 py-0.5 text-[0.625rem] font-mono uppercase text-muted-foreground">
              {epic.priority}
            </span>
          </div>
          <Link
            to="/"
            className="mt-1 block text-sm font-semibold text-foreground hover:text-primary"
          >
            {epic.title}
          </Link>
        </div>

        <div className="flex shrink-0 items-center gap-2">
          {epic.ready_to_close && (
            <Button
              size="sm"
              variant="default"
              aria-label={`Close epic: ${epic.title}`}
              onClick={() => onCloseEpic(epic.id)}
            >
              Close epic
            </Button>
          )}
          {children.length > 0 && (
            <Button
              size="sm"
              variant="ghost"
              onClick={() => setExpanded((v) => !v)}
              aria-expanded={expanded}
            >
              {expanded ? 'Hide' : `Show ${children.length}`}
            </Button>
          )}
        </div>
      </div>

      {/* Progress bars */}
      <div className="mt-3 flex flex-col gap-1.5">
        {/* Child roll-up */}
        <div className="flex items-center gap-2">
          <div className="h-1.5 flex-1 overflow-hidden rounded-full bg-muted">
            <div
              className="h-full rounded-full bg-primary"
              style={{ width: `${childPct}%` }}
            />
          </div>
          <span className="font-mono text-[0.625rem] text-muted-foreground">
            {epic.child_closed}/{epic.child_total} tasks
          </span>
        </div>

        {/* Criteria bar (only when there are criteria) */}
        {epic.criteria_total > 0 && (
          <div className="flex items-center gap-2">
            <div className="h-1.5 flex-1 overflow-hidden rounded-full bg-muted">
              <div
                className="h-full rounded-full bg-type-task"
                style={{ width: `${criteriaPct}%` }}
              />
            </div>
            <span className="font-mono text-[0.625rem] text-muted-foreground">
              {epic.criteria_met}/{epic.criteria_total} criteria
            </span>
          </div>
        )}
      </div>

      {/* Expanded child cards */}
      {expanded && children.length > 0 && (
        <div className="mt-4 grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
          {children.map((card) => (
            <div
              key={card.id}
              className="rounded-md border border-border/50 bg-background/60 p-3"
            >
              <BoardCardBody card={card} epicTitle={epic.title} />
            </div>
          ))}
        </div>
      )}
    </div>
  )
}

// ---------------------------------------------------------------------------
// EpicsView
// ---------------------------------------------------------------------------

export function EpicsView() {
  const { data, isLoading, error } = useEpicsQuery()
  const qc = useQueryClient()
  const [confirmId, setConfirmId] = React.useState<string | null>(null)
  const [closing, setClosing] = React.useState(false)

  async function handleConfirmClose() {
    if (!confirmId) return
    setClosing(true)
    try {
      await closeTicket(confirmId)
      await qc.invalidateQueries({ queryKey: ['epics'] })
      await qc.invalidateQueries({ queryKey: ['board'] })
      setConfirmId(null)
    } catch (err) {
      toastApiError(err)
    } finally {
      setClosing(false)
    }
  }

  if (isLoading) {
    return (
      <div className="flex flex-col gap-6 p-6">
        <PageHeader title="Epics" />
        <div className="flex flex-col gap-3">
          {[1, 2, 3].map((i) => (
            <Skeleton key={i} className="h-28 w-full rounded-lg" />
          ))}
        </div>
      </div>
    )
  }

  if (error) {
    return (
      <div className="flex flex-col gap-6 p-6">
        <PageHeader title="Epics" />
        <p className="text-sm text-destructive">Failed to load epics.</p>
      </div>
    )
  }

  const epics = data?.epics ?? []
  const children = data?.children ?? {}

  return (
    <div className="flex flex-col gap-6 p-6">
      <PageHeader
        title="Epics"
        subtitle={`${epics.length} epic${epics.length === 1 ? '' : 's'}`}
      />

      {epics.length === 0 ? (
        <EmptyState icon={Layers} title="No epics yet" description="Create an epic to group related tasks." />
      ) : (
        <div className="flex flex-col gap-3">
          {epics.map((epic) => (
            <EpicRow
              key={epic.id}
              epic={epic}
              children={children[epic.id] ?? []}
              onCloseEpic={setConfirmId}
            />
          ))}
        </div>
      )}

      {/* Close epic confirmation dialog */}
      <AlertDialog open={confirmId !== null} onOpenChange={(open) => { if (!open) setConfirmId(null) }}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle className="font-mono">Close epic?</AlertDialogTitle>
            <AlertDialogDescription>
              This will close the epic. All children are already closed. This action can be undone by reopening the ticket.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel onClick={() => setConfirmId(null)}>Cancel</AlertDialogCancel>
            <AlertDialogAction onClick={handleConfirmClose} disabled={closing}>
              {closing ? 'Closing…' : 'Close epic'}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  )
}

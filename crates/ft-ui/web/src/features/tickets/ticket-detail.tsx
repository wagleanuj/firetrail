/**
 * Ticket detail UI used both inside a `<Sheet>` (when navigating from `/`)
 * and as a full-page route (when the user lands on `/tickets/:id` directly).
 *
 * Responsibilities:
 *   - inline title editing (click to edit, ⏎ to save, esc to cancel)
 *   - priority dropdown
 *   - claim / unclaim / close actions
 *   - Tiptap description with explicit Edit / Save / Cancel
 *   - relations list + add-link modal
 */
import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { Loader2, Pencil, Link2, Plus, AlertTriangle } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Badge, type BadgeProps } from '@/components/ui/badge'
import { Input } from '@/components/ui/input'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from '@/components/ui/dialog'
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
import { fetchCriteria } from '@/features/audit/api'
import { RelativeTime } from '@/components/ui/relative-time'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Skeleton } from '@/components/ui/skeleton'
import { Label } from '@/components/ui/label'
import { cn } from '@/lib/utils'
import { Link } from '@tanstack/react-router'
import { recordDescription, recordClaim, type RecordWire } from '@/api/wire/record'
import type { TicketRelationKind } from '@/api/types/TicketRelationKind'
import type { Relation } from '@/api/wire/relation'
import { CriteriaEditor } from '@/features/audit/criteria-editor'
import { useTicketQuery } from './use-ticket-query'
import {
  useClaimTicket,
  useCloseTicket,
  useLinkTicket,
  useReopenTicket,
  useUnclaimTicket,
  useUpdateTicket,
} from './use-ticket-mutations'
import { DescriptionEditor } from './description-editor'
import { DocsPanel } from './docs-panel'
import { PriorityBadge } from './board'

const RELATION_KINDS: TicketRelationKind[] = [
  'blocks',
  'blocked-by',
  'parent-of',
  'child-of',
  'related-to',
  'duplicates',
  'supersedes',
  'fixed-by',
  'caused-by',
]

export function TicketDetail({ id }: { id: string }) {
  const { data, isLoading, error } = useTicketQuery(id)
  if (isLoading) return <DetailSkeleton />
  if (error) {
    return (
      <div className="text-sm text-destructive">
        Failed to load ticket: {(error as Error).message}
      </div>
    )
  }
  if (!data) return null
  return <DetailBody record={data.record} relations={data.relations} />
}

function DetailBody({ record, relations }: { record: RecordWire; relations: Relation[] }) {
  const env = record.envelope
  const claim = recordClaim(record)
  const update = useUpdateTicket(env.id)
  const claimMut = useClaimTicket(env.id)
  const unclaimMut = useUnclaimTicket(env.id)
  const closeMut = useCloseTicket(env.id)
  const reopenMut = useReopenTicket(env.id)
  const isClosed = env.status === 'closed' || env.status === 'deferred'
  const [forceClose, setForceClose] = useState(false)

  return (
    <div className="flex flex-col gap-6">
      {/* Header */}
      <div className="space-y-2">
        <div className="flex flex-wrap items-center gap-2">
          <WorkTypeBadge kind={env.kind} />
          <span className="text-xs font-mono text-muted-foreground">{env.id.slice(0, 14)}</span>
          <StatusPill status={env.status} />
        </div>
        <InlineTitle id={env.id} initial={env.title} pending={update.isPending} />
        <div className="flex flex-wrap items-center gap-3 pt-2">
          <PriorityBadge priority={env.priority} />
          <Select
            value={env.priority}
            onValueChange={(v) => update.mutate({ priority: v as 'p0' })}
          >
            <SelectTrigger className="h-7 w-24 text-xs">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {['p0', 'p1', 'p2', 'p3', 'p4'].map((p) => (
                <SelectItem key={p} value={p}>
                  {p.toUpperCase()}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <span className="text-xs text-muted-foreground">
            {env.owner ? `owner: ${env.owner.name}` : 'unowned'}
          </span>
        </div>
        <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-muted-foreground">
          <RelativeTime value={env.created_at} prefix="created" />
          <span>•</span>
          <RelativeTime value={env.updated_at} prefix="updated" />
          {env.closed_at && (
            <>
              <span>•</span>
              <RelativeTime value={env.closed_at} prefix="closed" />
            </>
          )}
        </div>
      </div>

      {/* Actions */}
      <div className="flex flex-wrap gap-2">
        {claim ? (
          <Button
            variant="outline"
            size="sm"
            onClick={() => unclaimMut.mutate()}
            disabled={unclaimMut.isPending}
          >
            {unclaimMut.isPending && <Loader2 className="mr-1 h-3 w-3 animate-spin" />}
            Unclaim
          </Button>
        ) : (
          <Button
            size="sm"
            onClick={() => claimMut.mutate()}
            disabled={claimMut.isPending || !canClaim(env.kind)}
            title={canClaim(env.kind) ? undefined : `${env.kind} records do not support claims`}
          >
            {claimMut.isPending && <Loader2 className="mr-1 h-3 w-3 animate-spin" />}
            Claim
          </Button>
        )}
        {isClosed ? (
          <Button
            variant="outline"
            size="sm"
            onClick={() => reopenMut.mutate()}
            disabled={reopenMut.isPending}
          >
            {reopenMut.isPending && <Loader2 className="mr-1 h-3 w-3 animate-spin" />}
            Reopen
          </Button>
        ) : (
          <>
            <Button
              variant="outline"
              size="sm"
              onClick={() => closeMut.mutate()}
              disabled={closeMut.isPending}
            >
              {closeMut.isPending && <Loader2 className="mr-1 h-3 w-3 animate-spin" />}
              Close
            </Button>
            <Button
              variant="destructive"
              size="sm"
              onClick={() => setForceClose(true)}
              disabled={closeMut.isPending}
              title="Force close — skips criteria validation"
            >
              <AlertTriangle className="mr-1 h-3 w-3" />
              Force close
            </Button>
          </>
        )}
      </div>
      <ForceCloseDialog
        recordId={env.id}
        open={forceClose}
        onOpenChange={setForceClose}
        onConfirm={(reason) => {
          closeMut.mutate(
            { force: true, reason: reason || undefined },
            { onSuccess: () => setForceClose(false) },
          )
        }}
        isPending={closeMut.isPending}
      />

      {/* Description */}
      <DescriptionPanel id={env.id} initial={recordDescription(record)} />

      {/* Acceptance criteria */}
      <CriteriaEditor recordId={env.id} />

      <div>
        <Link
          to="/audit/review/$recordId"
          params={{ recordId: env.id }}
          className="text-xs text-primary hover:underline"
        >
          View full audit review →
        </Link>
      </div>

      {/* Docs */}
      <DocsPanel ticketId={env.id} />

      {/* Relations */}
      <RelationsPanel id={env.id} relations={relations} />
    </div>
  )
}

function canClaim(kind: string): boolean {
  return kind === 'task' || kind === 'subtask' || kind === 'bug'
}

/**
 * Type-of-work pill for the detail header. Maps the record `kind` onto the
 * shared Badge `feature|bug|task|epic` variants (subtask reuses the task
 * accent). Presentational only.
 */
function WorkTypeBadge({ kind }: { kind: string }) {
  const variant: BadgeProps['variant'] =
    kind === 'feature'
      ? 'feature'
      : kind === 'bug'
        ? 'bug'
        : kind === 'epic'
          ? 'epic'
          : kind === 'task' || kind === 'subtask'
            ? 'task'
            : 'secondary'
  return (
    <Badge variant={variant} className="capitalize">
      {kind}
    </Badge>
  )
}

function StatusPill({ status }: { status: string }) {
  const tone =
    status === 'in_progress' || status === 'review'
      ? 'bg-primary/15 text-primary'
      : status === 'closed' || status === 'archived'
        ? 'bg-foreground/10 text-foreground'
        : 'bg-muted text-muted-foreground'
  return (
    <span className={cn('rounded-sm px-1.5 py-0.5 font-mono text-[0.625rem] font-semibold', tone)}>
      {status}
    </span>
  )
}

function InlineTitle({ id, initial, pending }: { id: string; initial: string; pending: boolean }) {
  const [editing, setEditing] = useState(false)
  const [draft, setDraft] = useState(initial)
  const update = useUpdateTicket(id)

  if (!editing) {
    return (
      <h1
        className="group flex cursor-text items-center gap-2 font-display text-xl font-semibold leading-snug tracking-tight"
        onClick={() => {
          setDraft(initial)
          setEditing(true)
        }}
      >
        {initial}
        <Pencil className="h-3.5 w-3.5 opacity-0 transition-opacity group-hover:opacity-60" />
        {pending && <Loader2 className="h-3.5 w-3.5 animate-spin opacity-60" />}
      </h1>
    )
  }
  return (
    <Input
      autoFocus
      value={draft}
      onChange={(e) => setDraft(e.target.value)}
      onKeyDown={(e) => {
        if (e.key === 'Enter') {
          e.preventDefault()
          if (draft.trim() && draft !== initial) {
            update.mutate({ title: draft.trim() })
          }
          setEditing(false)
        } else if (e.key === 'Escape') {
          setEditing(false)
        }
      }}
      onBlur={() => {
        if (draft.trim() && draft !== initial) {
          update.mutate({ title: draft.trim() })
        }
        setEditing(false)
      }}
      className="text-xl font-semibold"
    />
  )
}

function DescriptionPanel({ id, initial }: { id: string; initial: string }) {
  const [editing, setEditing] = useState(false)
  const [draft, setDraft] = useState(initial)
  const update = useUpdateTicket(id)

  if (!editing) {
    return (
      <div className="space-y-2">
        <div className="flex items-center justify-between">
          <Label>Description</Label>
          <Button
            type="button"
            size="sm"
            variant="ghost"
            className="h-7 gap-1.5 text-xs"
            onClick={() => {
              setDraft(initial)
              setEditing(true)
            }}
          >
            <Pencil className="h-3 w-3" />
            Edit
          </Button>
        </div>
        {initial.trim() ? (
          <DescriptionEditor value={initial} editable={false} />
        ) : (
          <p className="rounded-md border border-dashed border-border/60 px-3 py-4 text-sm text-muted-foreground">
            No description.
          </p>
        )}
      </div>
    )
  }

  return (
    <div className="space-y-2">
      <Label>Description</Label>
      <DescriptionEditor value={draft} onChange={setDraft} />
      <div className="flex justify-end gap-2">
        <Button type="button" size="sm" variant="ghost" onClick={() => setEditing(false)}>
          Cancel
        </Button>
        <Button
          type="button"
          size="sm"
          onClick={async () => {
            await update.mutateAsync({ description: draft })
            setEditing(false)
          }}
          disabled={update.isPending}
        >
          {update.isPending && <Loader2 className="mr-1 h-3 w-3 animate-spin" />}
          Save
        </Button>
      </div>
    </div>
  )
}

function RelationsPanel({ id, relations }: { id: string; relations: Relation[] }) {
  const [open, setOpen] = useState(false)
  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between">
        <Label>Relations</Label>
        <Button
          type="button"
          size="sm"
          variant="ghost"
          className="h-7 gap-1.5 text-xs"
          onClick={() => setOpen(true)}
        >
          <Plus className="h-3 w-3" />
          Add link
        </Button>
      </div>
      {relations.length === 0 ? (
        <p className="rounded-md border border-dashed border-border/60 px-3 py-3 text-sm text-muted-foreground">
          No relations.
        </p>
      ) : (
        <ul className="space-y-1">
          {relations.map((r) => {
            const outbound = r.from === id
            const other = outbound ? r.to : r.from
            return (
              <li
                key={`${r.from}-${r.to}-${r.kind}-${r.created_at}`}
                className="flex items-center gap-2 rounded-md border border-border/50 bg-background/60 px-3 py-2 font-mono text-xs"
              >
                <Link2 className="h-3.5 w-3.5 text-primary" />
                <span className="text-muted-foreground">
                  {outbound ? r.kind : `${r.kind} (in)`}
                </span>
                <span className="ml-auto truncate">{other.slice(0, 16)}</span>
              </li>
            )
          })}
        </ul>
      )}
      <AddLinkDialog id={id} open={open} onOpenChange={setOpen} />
    </div>
  )
}

function AddLinkDialog({
  id,
  open,
  onOpenChange,
}: {
  id: string
  open: boolean
  onOpenChange: (b: boolean) => void
}) {
  const [to, setTo] = useState('')
  const [kind, setKind] = useState<TicketRelationKind>('related-to')
  const link = useLinkTicket(id)
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle className="font-mono">Add relation</DialogTitle>
        </DialogHeader>
        <div className="space-y-3">
          <div className="space-y-1.5">
            <Label>Target ticket id</Label>
            <Input value={to} onChange={(e) => setTo(e.target.value)} placeholder="task:… or prefix" />
          </div>
          <div className="space-y-1.5">
            <Label>Kind</Label>
            <Select value={kind} onValueChange={(v) => setKind(v as TicketRelationKind)}>
              <SelectTrigger>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {RELATION_KINDS.map((k) => (
                  <SelectItem key={k} value={k}>
                    {k}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
        </div>
        <DialogFooter>
          <Button
            disabled={!to || link.isPending}
            onClick={async () => {
              await link.mutateAsync({ to, kind })
              setTo('')
              onOpenChange(false)
            }}
          >
            {link.isPending && <Loader2 className="mr-1 h-3 w-3 animate-spin" />}
            Link
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

/**
 * Force-close confirms the destructive close-skip-criteria operation. Surfaces
 * the unchecked acceptance criteria so the user sees exactly what will be
 * bypassed.
 */
function ForceCloseDialog({
  recordId,
  open,
  onOpenChange,
  onConfirm,
  isPending,
}: {
  recordId: string
  open: boolean
  onOpenChange: (b: boolean) => void
  onConfirm: (reason: string) => void
  isPending: boolean
}) {
  const [reason, setReason] = useState('')
  const criteria = useQuery({
    queryKey: ['audit-criteria', recordId] as const,
    queryFn: () => fetchCriteria(recordId),
    enabled: open,
  })
  const unchecked = (criteria.data?.items ?? []).filter((it) => !it.checked)
  return (
    <AlertDialog open={open} onOpenChange={onOpenChange}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle className="font-mono">Force close ticket?</AlertDialogTitle>
          <AlertDialogDescription>
            Force-close skips acceptance criteria validation. The ticket is closed
            even if some criteria remain unchecked. Use only when blockers cannot
            be satisfied (e.g. wont-fix, duplicate).
          </AlertDialogDescription>
        </AlertDialogHeader>
        {unchecked.length > 0 && (
          <div className="space-y-1 rounded-md border border-amber-400/40 bg-amber-400/5 p-3 text-xs">
            <p className="font-mono uppercase tracking-wider text-amber-300">
              {unchecked.length} unchecked criteri
              {unchecked.length === 1 ? 'on' : 'a'}
            </p>
            <ul className="space-y-0.5 text-foreground/80">
              {unchecked.slice(0, 8).map((it) => (
                <li key={it.id} className="truncate">
                  <code className="mr-1 font-mono text-[0.65rem] text-muted-foreground">
                    {it.id}
                  </code>
                  {it.text}
                </li>
              ))}
              {unchecked.length > 8 && (
                <li className="text-muted-foreground">
                  … and {unchecked.length - 8} more
                </li>
              )}
            </ul>
          </div>
        )}
        <div className="space-y-1.5">
          <Label>Reason (optional)</Label>
          <Input
            value={reason}
            onChange={(e) => setReason(e.target.value)}
            placeholder="Short rationale…"
          />
        </div>
        <AlertDialogFooter>
          <AlertDialogCancel onClick={() => onOpenChange(false)}>Cancel</AlertDialogCancel>
          <AlertDialogAction
            disabled={isPending}
            onClick={(e) => {
              e.preventDefault()
              onConfirm(reason)
            }}
            className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
          >
            {isPending && <Loader2 className="mr-1 h-3 w-3 animate-spin" />}
            Force close
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}

function DetailSkeleton() {
  return (
    <div className="space-y-4">
      <Skeleton className="h-4 w-40" />
      <Skeleton className="h-8 w-3/4" />
      <Skeleton className="h-24 w-full" />
      <Skeleton className="h-16 w-full" />
    </div>
  )
}

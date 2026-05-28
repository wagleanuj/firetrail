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
import { Loader2, Pencil, Link2, Plus } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from '@/components/ui/dialog'
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
  useUnclaimTicket,
  useUpdateTicket,
} from './use-ticket-mutations'
import { DescriptionEditor } from './description-editor'
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

  return (
    <div className="flex flex-col gap-6">
      {/* Header */}
      <div className="space-y-2">
        <div className="flex items-center gap-2 font-mono text-xs uppercase tracking-wider text-muted-foreground">
          <span>{env.id.slice(0, 14)}</span>
          <span>•</span>
          <span>{env.kind}</span>
          <span>•</span>
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
        <Button
          variant="outline"
          size="sm"
          onClick={() => closeMut.mutate()}
          disabled={closeMut.isPending || env.status === 'closed'}
        >
          {closeMut.isPending && <Loader2 className="mr-1 h-3 w-3 animate-spin" />}
          Close
        </Button>
      </div>

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

      {/* Relations */}
      <RelationsPanel id={env.id} relations={relations} />
    </div>
  )
}

function canClaim(kind: string): boolean {
  return kind === 'task' || kind === 'subtask' || kind === 'bug'
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
        className="group flex cursor-text items-center gap-2 text-xl font-semibold leading-tight"
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

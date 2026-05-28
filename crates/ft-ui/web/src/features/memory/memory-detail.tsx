/**
 * Memory detail page.
 *
 * Memory records are **immutable** post-create — the W2-B backend exposes
 * no PATCH/update route (only list / show / create / capture / search /
 * salvage). So this page is read-only: it renders the title, the kind/trust
 * metadata, and the Markdown body via the shared editor in `editable=false`
 * mode. A hint at the top documents the immutability for users who might
 * expect an Edit button.
 *
 * The "Find similar" action deep-links to `/memory/search?similarTo=<id>`,
 * which short-circuits the search page into the `/api/memory/similar/:id`
 * branch.
 */
import { Link, useNavigate } from '@tanstack/react-router'
import { Sparkles, Lock } from 'lucide-react'
import { RelativeTime } from '@/components/ui/relative-time'
import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import { Skeleton } from '@/components/ui/skeleton'
import { Separator } from '@/components/ui/separator'
import { MarkdownEditor } from '@/components/markdown-editor'
import { recordDescription, recordTrust, type RecordWire } from '@/api/wire/record'
import { useMemoryQuery } from './use-memory-query'
import { KindBadge } from './memory-list'
import { TrustActions } from '@/features/trust/trust-actions'
import { CriteriaEditor } from '@/features/audit/criteria-editor'

interface MemoryDetailProps {
  id: string
}

export function MemoryDetail({ id }: MemoryDetailProps) {
  const { data, isLoading, error } = useMemoryQuery(id)
  const navigate = useNavigate()

  if (isLoading) return <DetailSkeleton />
  if (error) {
    return (
      <div className="p-6 text-sm text-destructive">
        Failed to load memory: {(error as Error).message}
      </div>
    )
  }
  if (!data) return null

  const { record } = data
  const env = record.envelope
  const body = readMemoryBody(record)
  const { trust, riskClass } = recordTrust(record)

  return (
    <div className="mx-auto max-w-3xl space-y-6 p-6">
      <div className="space-y-2">
        <div className="flex items-center gap-2 font-mono text-xs uppercase tracking-wider text-muted-foreground">
          <Link to="/memory" className="hover:text-primary">
            memory
          </Link>
          <span>/</span>
          <span>{env.id.slice(0, 14)}</span>
        </div>
        <div className="flex flex-wrap items-center gap-3">
          <KindBadge kind={env.kind} />
          <h1 className="text-2xl font-semibold leading-tight">{env.title}</h1>
        </div>
        <div className="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
          {env.owner && <span>owner: {env.owner.name}</span>}
          {env.owner && <span>•</span>}
          <RelativeTime value={env.updated_at} prefix="updated" />
          {env.closed_at && (
            <>
              <span>•</span>
              <RelativeTime value={env.closed_at} prefix="closed" />
            </>
          )}
        </div>
      </div>

      <div className="flex flex-wrap gap-2">
        <Button
          size="sm"
          variant="outline"
          className="gap-2"
          onClick={() =>
            navigate({
              to: '/memory/search',
              search: { similarTo: env.id, mode: 'auto' },
            })
          }
        >
          <Sparkles className="h-3.5 w-3.5" />
          Find similar
        </Button>
      </div>

      <Separator />

      {/* Immutability hint */}
      <div className="flex items-start gap-2 rounded-md border border-border/60 bg-muted/40 px-3 py-2 text-xs text-muted-foreground">
        <Lock className="mt-0.5 h-3.5 w-3.5 text-primary" />
        <span>
          Memories are immutable. Supersede with a new record if the content
          needs to change — the salvage workflow and trust transitions are how
          history evolves.
        </span>
      </div>

      <div className="space-y-2">
        <Label>Body</Label>
        {body.trim() ? (
          <MarkdownEditor value={body} editable={false} />
        ) : (
          <p className="rounded-md border border-dashed border-border/60 px-3 py-4 text-sm text-muted-foreground">
            No body text.
          </p>
        )}
      </div>

      <Separator />

      <TrustActions recordId={env.id} trustState={trust} riskClass={riskClass} />

      <Separator />

      <CriteriaEditor recordId={env.id} />

      <div className="pt-2">
        <Link
          to="/audit/review/$recordId"
          params={{ recordId: env.id }}
          className="text-xs text-primary hover:underline"
        >
          View full audit review →
        </Link>
      </div>
    </div>
  )
}

function DetailSkeleton() {
  return (
    <div className="mx-auto max-w-3xl space-y-4 p-6">
      <Skeleton className="h-4 w-40" />
      <Skeleton className="h-9 w-2/3" />
      <Skeleton className="h-4 w-1/3" />
      <Skeleton className="h-40 w-full" />
    </div>
  )
}

/**
 * Memory records carry their long-form text in a kind-specific body field.
 * `recordDescription()` from the tickets wire only knows about the ticket
 * bodies (epic/task/subtask/bug); memory bodies live under different keys
 * (`memory.body`, `finding.details`, `gotcha.details`, decision's
 * `context+decision+consequences`, etc.). We accept any of those.
 */
function readMemoryBody(record: RecordWire): string {
  // Tickets path already handles task/etc — try that first for incident
  // (which may not have a description in this shape).
  const tickets = recordDescription(record)
  if (tickets) return tickets

  const body = record.body as Record<string, unknown>
  // Direct body string fields by kind.
  const candidates: Array<[string, string]> = [
    ['memory', 'body'],
    ['finding', 'details'],
    ['gotcha', 'details'],
    ['runbook', 'summary'],
    ['incident', 'summary'],
  ]
  for (const [kind, field] of candidates) {
    const inner = body[kind] as Record<string, unknown> | undefined
    const v = inner?.[field]
    if (typeof v === 'string' && v.trim()) return v
  }
  // Decision concatenates context/decision/consequences.
  const decision = body['decision'] as
    | { context?: string; decision?: string; consequences?: string | null }
    | undefined
  if (decision) {
    const parts: string[] = []
    if (decision.context) parts.push(`## Context\n\n${decision.context}`)
    if (decision.decision) parts.push(`## Decision\n\n${decision.decision}`)
    if (decision.consequences) parts.push(`## Consequences\n\n${decision.consequences}`)
    if (parts.length) return parts.join('\n\n')
  }
  return ''
}

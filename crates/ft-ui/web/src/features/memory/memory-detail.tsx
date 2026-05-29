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
import { PageHeader } from '@/components/page-header'
import { MarkdownEditor } from '@/components/markdown-editor'
import { recordDescription, recordTrust, type RecordWire } from '@/api/wire/record'
import { useMemoryQuery } from './use-memory-query'
import { KindBadge } from './memory-list'
import { TrustActions } from '@/features/trust/trust-actions'
import { TrustBadge } from '@/features/trust/trust-badge'
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
    <div className="mx-auto max-w-3xl space-y-6 px-6 py-5">
      <div className="flex items-center gap-2 font-mono text-xs uppercase tracking-wider text-muted-foreground">
        <Link to="/memory" className="transition-colors hover:text-primary">
          memory
        </Link>
        <span>/</span>
        <span>{env.id.slice(0, 14)}</span>
      </div>

      <PageHeader
        title={env.title}
        subtitle={
          <div className="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
            <KindBadge kind={env.kind} />
            <TrustBadge state={trust} />
            {riskClass && (
              <span className="rounded-full bg-muted px-2 py-0.5 font-mono text-[0.625rem] uppercase tracking-wider">
                {riskClass}
              </span>
            )}
            <span className="text-muted-foreground/50">·</span>
            {env.owner && (
              <>
                <span>owner: {env.owner.name}</span>
                <span className="text-muted-foreground/50">·</span>
              </>
            )}
            <RelativeTime value={env.updated_at} prefix="updated" />
            {env.closed_at && (
              <>
                <span className="text-muted-foreground/50">·</span>
                <RelativeTime value={env.closed_at} prefix="closed" />
              </>
            )}
          </div>
        }
        actions={
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
        }
      />

      <Separator />

      {/* Immutability hint */}
      <div className="flex items-start gap-2 rounded-lg border border-border bg-muted/40 px-3 py-2 text-xs text-muted-foreground">
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
          <p className="rounded-lg border border-dashed border-border px-3 py-4 text-sm text-muted-foreground">
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
 * RecordBody is internally tagged (`{"kind":"incident","summary":"...",...}`),
 * so we dispatch on `body.kind` and read fields directly off `body`.
 */
function readMemoryBody(record: RecordWire): string {
  const tickets = recordDescription(record)
  if (tickets) return tickets

  const body = record.body
  switch (body.kind) {
    case 'memory':
      return typeof body.body === 'string' ? body.body : ''
    case 'finding':
    case 'gotcha':
      return typeof body.details === 'string' ? body.details : ''
    case 'runbook':
      return typeof body.summary === 'string' ? body.summary : ''
    case 'incident': {
      const parts: string[] = []
      if (typeof body.summary === 'string' && body.summary.trim()) parts.push(body.summary)
      if (typeof body.root_cause === 'string' && body.root_cause.trim()) {
        parts.push(`## Root cause\n\n${body.root_cause}`)
      }
      return parts.join('\n\n')
    }
    case 'decision': {
      const parts: string[] = []
      if (typeof body.context === 'string' && body.context) parts.push(`## Context\n\n${body.context}`)
      if (typeof body.decision === 'string' && body.decision) parts.push(`## Decision\n\n${body.decision}`)
      if (typeof body.consequences === 'string' && body.consequences) {
        parts.push(`## Consequences\n\n${body.consequences}`)
      }
      return parts.join('\n\n')
    }
    default:
      return ''
  }
}

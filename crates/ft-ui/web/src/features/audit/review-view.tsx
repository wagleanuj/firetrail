/**
 * Review viewer for a single record — surfaces the audit summary with
 * acceptance criteria, evidence and history. Reached via deep-link from
 * "View review" affordances on record detail pages.
 */
import { useQuery } from '@tanstack/react-query'
import { Link } from '@tanstack/react-router'
import { CheckCircle2, XCircle } from 'lucide-react'
import { RelativeTime } from '@/components/ui/relative-time'
import { Skeleton } from '@/components/ui/skeleton'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { Separator } from '@/components/ui/separator'
import { PageHeader } from '@/components/page-header'
import { cn } from '@/lib/utils'
import { fetchReview } from './api'

export function ReviewView({ recordId }: { recordId: string }) {
  const { data, isLoading, error } = useQuery({
    queryKey: ['audit-review', recordId] as const,
    queryFn: () => fetchReview(recordId),
    enabled: !!recordId,
  })

  if (isLoading) {
    return (
      <div className="space-y-4">
        <Skeleton className="h-7 w-2/3" />
        <Skeleton className="h-4 w-1/2" />
        <Skeleton className="h-40 w-full" />
        <Skeleton className="h-40 w-full" />
      </div>
    )
  }
  if (error) {
    return (
      <p className="rounded-[var(--radius)] border border-danger/30 bg-danger/10 px-3 py-2 text-sm text-danger">
        Failed to load review: {(error as Error).message}
      </p>
    )
  }
  if (!data) return null

  const ticketKinds = ['epic', 'task', 'subtask', 'bug']
  const isTicket = ticketKinds.some((k) => data.id.startsWith(`${k}:`))

  return (
    <div className="space-y-6">
      <div className="flex items-center gap-2 font-mono text-xs uppercase tracking-wider text-muted-foreground">
        <Link to="/audit" className="hover:text-primary">
          audit
        </Link>
        <span>/</span>
        <span>review</span>
        <span>/</span>
        <span className="text-foreground">{data.id}</span>
      </div>

      <PageHeader
        title={data.title}
        subtitle={
          <div className="flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-muted-foreground">
            <span className="rounded-sm bg-primary/15 px-1.5 py-0.5 font-mono text-[0.625rem] font-semibold uppercase tracking-wider text-primary">
              {data.kind}
            </span>
            <span>status: {data.status}</span>
            <span>•</span>
            <span>priority: {data.priority}</span>
            {data.owner && (
              <>
                <span>•</span>
                <span>owner: {data.owner}</span>
              </>
            )}
            {data.trustState && (
              <>
                <span>•</span>
                <span>trust: {data.trustState}</span>
              </>
            )}
            {data.riskClass && (
              <>
                <span>•</span>
                <span>risk: {data.riskClass}</span>
                {data.highStakes && <span className="text-warning">high-stakes</span>}
              </>
            )}
          </div>
        }
        actions={
          <Link
            to={isTicket ? '/tickets/$id' : '/memory/$id'}
            params={{ id: data.id }}
            className="text-sm text-primary hover:underline"
          >
            Open record →
          </Link>
        }
      />

      <div
        className={cn(
          'inline-flex items-center gap-2 rounded-[var(--radius)] border px-3 py-1.5 text-xs',
          data.chainValid
            ? 'border-success/30 bg-success/10 text-success'
            : 'border-danger/30 bg-danger/10 text-danger',
        )}
      >
        {data.chainValid ? <CheckCircle2 className="h-3.5 w-3.5" /> : <XCircle className="h-3.5 w-3.5" />}
        Chain: {data.chainStatus}
      </div>

      <Separator />

      <section className="space-y-2">
        <h2 className="text-sm font-medium uppercase tracking-wide text-muted-foreground">
          Acceptance criteria
        </h2>
        {data.acceptanceCriteria.length === 0 ? (
          <p className="rounded-[var(--radius)] border border-dashed border-border px-3 py-3 text-sm text-muted-foreground">
            None.
          </p>
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead className="w-16">Id</TableHead>
                <TableHead>Text</TableHead>
                <TableHead className="w-24 text-center">Checked</TableHead>
                <TableHead>Evidence</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {data.acceptanceCriteria.map((ac) => (
                <TableRow key={ac.id}>
                  <TableCell>
                    <code className="font-mono text-xs">{ac.id}</code>
                  </TableCell>
                  <TableCell className="text-sm">{ac.text}</TableCell>
                  <TableCell className="text-center">
                    {ac.status === 'checked' ? (
                      <CheckCircle2 className="mx-auto h-4 w-4 text-success" />
                    ) : (
                      <XCircle className="mx-auto h-4 w-4 text-muted-foreground" />
                    )}
                  </TableCell>
                  <TableCell>
                    {ac.evidenceUrl ? (
                      <a
                        href={ac.evidenceUrl}
                        target="_blank"
                        rel="noreferrer noopener"
                        className="font-mono text-xs text-primary hover:underline"
                      >
                        {ac.evidenceUrl}
                      </a>
                    ) : (
                      <span className="text-muted-foreground">—</span>
                    )}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        )}
      </section>

      <section className="space-y-2">
        <h2 className="text-sm font-medium uppercase tracking-wide text-muted-foreground">
          Evidence
        </h2>
        {data.evidence.length === 0 ? (
          <p className="rounded-[var(--radius)] border border-dashed border-border px-3 py-3 text-sm text-muted-foreground">
            None.
          </p>
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Kind</TableHead>
                <TableHead>Url</TableHead>
                <TableHead>Note</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {data.evidence.map((e, i) => (
                <TableRow key={i}>
                  <TableCell>
                    <code className="font-mono text-xs">{(e as { kind?: string }).kind ?? '—'}</code>
                  </TableCell>
                  <TableCell>
                    <code className="break-all font-mono text-xs">
                      {(e as { url?: string }).url ?? '—'}
                    </code>
                  </TableCell>
                  <TableCell className="text-sm">{(e as { note?: string }).note ?? ''}</TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        )}
      </section>

      <section className="space-y-2">
        <h2 className="text-sm font-medium uppercase tracking-wide text-muted-foreground">
          History
        </h2>
        {data.history.length === 0 ? (
          <p className="rounded-[var(--radius)] border border-dashed border-border px-3 py-3 text-sm text-muted-foreground">
            No history entries.
          </p>
        ) : (
          <ul className="space-y-1.5">
            {data.history.map((h, i) => {
              const at = (h as { at?: string }).at
              const event = (h as { event?: string }).event ?? ''
              const actor = (h as { actor?: string }).actor ?? ''
              return (
                <li key={i} className="rounded-[var(--radius)] border border-border bg-card p-2 text-xs">
                  <span className="font-mono">
                    <RelativeTime value={at} className="text-muted-foreground" />
                    {event && <> · {event}</>}
                  </span>
                  {actor && <span className="ml-2 text-muted-foreground">{actor}</span>}
                </li>
              )
            })}
          </ul>
        )}
      </section>

      <Separator />

      <section className="rounded-[var(--radius)] border border-border bg-surface-2 p-3 text-sm shadow-elevation-1">
        <span className="text-sm font-medium uppercase tracking-wide text-muted-foreground">
          Suggested next action
        </span>
        <p className="mt-1">{data.suggestedNextAction}</p>
      </section>
    </div>
  )
}

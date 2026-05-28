/**
 * Review viewer for a single record — surfaces the audit summary with
 * acceptance criteria, evidence and history. Reached via deep-link from
 * "View review" affordances on record detail pages.
 */
import { useQuery } from '@tanstack/react-query'
import { Link } from '@tanstack/react-router'
import { CheckCircle2, XCircle } from 'lucide-react'
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
import { cn } from '@/lib/utils'
import { fetchReview } from './api'

export function ReviewView({ recordId }: { recordId: string }) {
  const { data, isLoading, error } = useQuery({
    queryKey: ['audit-review', recordId] as const,
    queryFn: () => fetchReview(recordId),
    enabled: !!recordId,
  })

  if (isLoading) return <Skeleton className="h-96 w-full" />
  if (error) {
    return (
      <p className="text-sm text-destructive">
        Failed to load review: {(error as Error).message}
      </p>
    )
  }
  if (!data) return null

  const ticketKinds = ['epic', 'task', 'subtask', 'bug']
  const isTicket = ticketKinds.some((k) => data.id.startsWith(`${k}:`))

  return (
    <div className="space-y-6">
      <header className="space-y-2">
        <div className="flex items-center gap-2 font-mono text-xs uppercase tracking-wider text-muted-foreground">
          <Link to="/audit" className="hover:text-primary">
            audit
          </Link>
          <span>/</span>
          <span>review</span>
          <span>/</span>
          <span>{data.id}</span>
        </div>
        <div className="flex items-baseline gap-3">
          <span className="rounded-sm bg-primary/15 px-1.5 py-0.5 font-mono text-[0.625rem] font-semibold uppercase tracking-wider text-primary">
            {data.kind}
          </span>
          <h1 className="text-xl font-semibold">{data.title}</h1>
        </div>
        <div className="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
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
              {data.highStakes && <span className="text-amber-300">high-stakes</span>}
            </>
          )}
        </div>
        <div
          className={cn(
            'inline-flex items-center gap-2 rounded-md border px-3 py-1.5 text-xs',
            data.chainValid
              ? 'border-primary/30 bg-primary/5 text-primary'
              : 'border-destructive/30 bg-destructive/5 text-destructive',
          )}
        >
          {data.chainValid ? <CheckCircle2 className="h-3.5 w-3.5" /> : <XCircle className="h-3.5 w-3.5" />}
          Chain: {data.chainStatus}
        </div>
        <div className="pt-2">
          <Link
            to={isTicket ? '/tickets/$id' : '/memory/$id'}
            params={{ id: data.id }}
            className="text-sm text-primary hover:underline"
          >
            Open record →
          </Link>
        </div>
      </header>

      <Separator />

      <section className="space-y-2">
        <h2 className="font-mono text-xs uppercase tracking-wider text-muted-foreground">
          Acceptance criteria
        </h2>
        {data.acceptanceCriteria.length === 0 ? (
          <p className="rounded-md border border-dashed border-border/60 px-3 py-3 text-sm text-muted-foreground">
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
                      <CheckCircle2 className="mx-auto h-4 w-4 text-primary" />
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
        <h2 className="font-mono text-xs uppercase tracking-wider text-muted-foreground">
          Evidence
        </h2>
        {data.evidence.length === 0 ? (
          <p className="rounded-md border border-dashed border-border/60 px-3 py-3 text-sm text-muted-foreground">
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
        <h2 className="font-mono text-xs uppercase tracking-wider text-muted-foreground">
          History
        </h2>
        {data.history.length === 0 ? (
          <p className="rounded-md border border-dashed border-border/60 px-3 py-3 text-sm text-muted-foreground">
            No history entries.
          </p>
        ) : (
          <ul className="space-y-1.5">
            {data.history.map((h, i) => (
              <li key={i} className="rounded-md border border-border/60 bg-background/60 p-2 text-xs">
                <span className="font-mono">
                  {(h as { at?: string }).at ?? ''} · {(h as { event?: string }).event ?? ''}
                </span>
                <span className="ml-2 text-muted-foreground">
                  {(h as { actor?: string }).actor ?? ''}
                </span>
              </li>
            ))}
          </ul>
        )}
      </section>

      <Separator />

      <section className="rounded-md border border-border/70 bg-background/60 p-3 text-sm">
        <span className="font-mono text-xs uppercase tracking-wider text-muted-foreground">
          Suggested next action
        </span>
        <p className="mt-1">{data.suggestedNextAction}</p>
      </section>
    </div>
  )
}

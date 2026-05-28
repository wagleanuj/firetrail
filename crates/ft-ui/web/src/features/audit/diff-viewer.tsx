/**
 * Diff viewer. Inputs are synced to the route's search params so a URL can
 * deep-link to a specific base/head/scope/memoryOnly view.
 */
import { Link } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { Loader2, GitCommitHorizontal } from 'lucide-react'
import type { DiffChange } from '@/api/types/DiffChange'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Skeleton } from '@/components/ui/skeleton'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { cn } from '@/lib/utils'
import { fetchDiff } from './api'

interface DiffViewerProps {
  base: string
  head: string
  memoryOnly: boolean
  scope: string
  onChange: (next: { base?: string; head?: string; memoryOnly?: boolean; scope?: string }) => void
}

export function DiffViewer({ base, head, memoryOnly, scope, onChange }: DiffViewerProps) {
  const enabled = !!(base && head)
  const { data, isLoading, error, refetch, isFetching } = useQuery({
    queryKey: ['audit-diff', base, head, memoryOnly, scope] as const,
    queryFn: () => fetchDiff({ base, head, memoryOnly, scope: scope || undefined }),
    enabled,
    staleTime: 5_000,
  })

  return (
    <div className="space-y-4">
      <header className="flex items-end justify-between gap-3 rounded-md border border-border/70 bg-background/60 p-3">
        <div className="grid flex-1 grid-cols-2 gap-3 lg:grid-cols-4">
          <div className="space-y-1.5">
            <Label className="text-xs">Base</Label>
            <Input value={base} onChange={(e) => onChange({ base: e.target.value })} placeholder="main" />
          </div>
          <div className="space-y-1.5">
            <Label className="text-xs">Head</Label>
            <Input value={head} onChange={(e) => onChange({ head: e.target.value })} placeholder="HEAD" />
          </div>
          <div className="space-y-1.5">
            <Label className="text-xs">Scope (prefix)</Label>
            <Input value={scope} onChange={(e) => onChange({ scope: e.target.value })} placeholder="optional" />
          </div>
          <div className="flex items-end gap-2">
            <label className="flex items-center gap-2 text-sm">
              <input
                type="checkbox"
                className="h-4 w-4 accent-primary"
                checked={memoryOnly}
                onChange={(e) => onChange({ memoryOnly: e.target.checked })}
              />
              Memory only
            </label>
          </div>
        </div>
        <Button size="sm" onClick={() => refetch()} disabled={!enabled || isFetching} className="gap-2">
          {isFetching ? <Loader2 className="h-3 w-3 animate-spin" /> : <GitCommitHorizontal className="h-3 w-3" />}
          Run diff
        </Button>
      </header>

      {isLoading && <Skeleton className="h-40 w-full" />}
      {error && (
        <p className="text-sm text-destructive">
          Failed to load diff: {(error as Error).message}
        </p>
      )}
      {data && data.rows.length === 0 && (
        <p className="rounded-md border border-dashed border-border/60 px-3 py-6 text-center text-sm text-muted-foreground">
          No differences between <code className="font-mono">{base}</code> and{' '}
          <code className="font-mono">{head}</code>.
        </p>
      )}
      {data && data.rows.length > 0 && (
        <Table data-testid="diff-rows">
          <TableHeader>
            <TableRow>
              <TableHead>Path</TableHead>
              <TableHead className="w-24">Kind</TableHead>
              <TableHead className="w-28">Change</TableHead>
              <TableHead className="w-40">Scope</TableHead>
              <TableHead>Title</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {data.rows.map((row) => (
              <TableRow key={`${row.path}-${row.id ?? ''}`}>
                <TableCell>
                  <code className="break-all font-mono text-xs">{row.path}</code>
                </TableCell>
                <TableCell>
                  <code className="font-mono text-xs">{row.kind ?? row.class}</code>
                </TableCell>
                <TableCell>
                  <ChangePill change={row.change} />
                </TableCell>
                <TableCell>
                  {row.scope ? (
                    <Link
                      to="/scope/$id"
                      params={{ id: row.scope }}
                      className="font-mono text-xs text-primary hover:underline"
                    >
                      {row.scope}
                    </Link>
                  ) : (
                    <span className="text-muted-foreground">—</span>
                  )}
                </TableCell>
                <TableCell>
                  {row.id ? (
                    <RecordLink id={row.id} title={row.title} />
                  ) : (
                    <span className="text-sm text-muted-foreground">{row.title}</span>
                  )}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      )}
    </div>
  )
}

function ChangePill({ change }: { change: DiffChange }) {
  const text = typeof change === 'string' ? change : JSON.stringify(change)
  const tone =
    text === 'added'
      ? 'bg-primary/15 text-primary'
      : text === 'removed'
        ? 'bg-destructive/15 text-destructive'
        : 'bg-muted text-muted-foreground'
  return (
    <span className={cn('rounded px-1.5 py-0.5 font-mono text-[0.625rem] uppercase tracking-wider', tone)}>
      {text}
    </span>
  )
}

function RecordLink({ id, title }: { id: string; title: string | null }) {
  const ticketKinds = ['epic', 'task', 'subtask', 'bug']
  const isTicket = ticketKinds.some((k) => id.startsWith(`${k}:`))
  const to = isTicket ? '/tickets/$id' : '/memory/$id'
  return (
    <Link to={to} params={{ id }} className="text-sm text-primary hover:underline">
      {title ?? id}
    </Link>
  )
}

/**
 * Relations graph viewer.
 *
 * Library choice: `react-force-graph-2d`. It's a small canvas-based wrapper
 * around `d3-force` — no WebGL dependency (keeps the bundle modest) and the
 * imperative `ref` API lets us hook click → navigate without React tree
 * thrash.
 *
 * The component lazily imports the heavy module behind a dynamic `import()`
 * so it lands in its own chunk (see vite.config.ts manualChunks).
 */
import { lazy, Suspense, useMemo, useRef } from 'react'
import { useQuery } from '@tanstack/react-query'
import { useNavigate } from '@tanstack/react-router'
import { Loader2, Network } from 'lucide-react'
import type { GraphDirectionInput } from '@/api/types/GraphDirectionInput'
import type { GraphNode } from '@/api/types/GraphNode'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Skeleton } from '@/components/ui/skeleton'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { fetchGraph } from './api'

const ForceGraph2D = lazy(() => import('react-force-graph-2d'))

const DIRECTIONS: GraphDirectionInput[] = ['up', 'down', 'both']

// Stable colours by kind. Anything not in the map falls back to muted-foreground.
const KIND_COLOR: Record<string, string> = {
  epic: '#a78bfa',
  task: '#22d3ee',
  subtask: '#67e8f9',
  bug: '#f87171',
  incident: '#f59e0b',
  finding: '#fbbf24',
  runbook: '#34d399',
  decision: '#60a5fa',
  gotcha: '#fb923c',
  memory: '#94a3b8',
}

interface GraphViewerProps {
  id: string
  direction: GraphDirectionInput
  depth: number
  onChange: (next: { id?: string; direction?: GraphDirectionInput; depth?: number }) => void
}

export function GraphViewer({ id, direction, depth, onChange }: GraphViewerProps) {
  const navigate = useNavigate()
  const enabled = !!id
  const { data, isLoading, error, refetch, isFetching } = useQuery({
    queryKey: ['audit-graph', id, direction, depth] as const,
    queryFn: () => fetchGraph({ id, direction, depth }),
    enabled,
    staleTime: 5_000,
  })

  const ref = useRef<unknown>(null)

  const graphData = useMemo(() => {
    if (!data) return { nodes: [], links: [] }
    return {
      nodes: data.nodes.map((n) => ({ ...n, color: KIND_COLOR[n.kind] ?? '#94a3b8' })),
      links: data.edges.map((e) => ({ source: e.from, target: e.to, kind: e.kind })),
    }
  }, [data])

  return (
    <div className="space-y-4">
      <header className="flex flex-wrap items-end gap-3 rounded-md border border-border/70 bg-background/60 p-3">
        <div className="flex-1 space-y-1.5">
          <Label className="text-xs">Root id</Label>
          <Input value={id} onChange={(e) => onChange({ id: e.target.value })} placeholder="task:… / memory:…" />
        </div>
        <div className="space-y-1.5">
          <Label className="text-xs">Direction</Label>
          <Select value={direction} onValueChange={(v) => onChange({ direction: v as GraphDirectionInput })}>
            <SelectTrigger className="w-32">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {DIRECTIONS.map((d) => (
                <SelectItem key={d} value={d}>
                  {d}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <div className="space-y-1.5">
          <Label className="text-xs">Depth ({depth})</Label>
          <input
            type="range"
            min={1}
            max={5}
            value={depth}
            onChange={(e) => onChange({ depth: Number(e.target.value) })}
            className="w-32 accent-primary"
          />
        </div>
        <Button size="sm" onClick={() => refetch()} disabled={!enabled || isFetching} className="gap-2">
          {isFetching ? <Loader2 className="h-3 w-3 animate-spin" /> : <Network className="h-3 w-3" />}
          Walk
        </Button>
      </header>

      {isLoading && <Skeleton className="h-96 w-full" />}
      {error && (
        <p className="text-sm text-destructive">
          Failed to load graph: {(error as Error).message}
        </p>
      )}

      {data && data.nodes.length === 0 && (
        <p className="rounded-md border border-dashed border-border/60 px-3 py-6 text-center text-sm text-muted-foreground">
          {data.reason ?? `No relations found from ${id}.`}
        </p>
      )}

      {data && data.nodes.length > 0 && (
        <div className="space-y-2">
          <Legend nodes={data.nodes} />
          <div
            className="rounded-md border border-border/70 bg-background/60"
            data-testid="force-graph-container"
            style={{ height: 480 }}
          >
            <Suspense fallback={<Skeleton className="h-full w-full" />}>
              {/* eslint-disable @typescript-eslint/no-explicit-any */}
              <ForceGraph2D
                ref={ref as any}
                graphData={graphData as any}
                nodeLabel={((n: any) => `${n.kind}: ${n.title || n.id}`) as any}
                nodeColor={((n: any) => n.color ?? '#94a3b8') as any}
                linkLabel={((l: any) => l.kind) as any}
                linkColor={() => 'rgba(148, 163, 184, 0.4)'}
                linkDirectionalArrowLength={3}
                linkDirectionalArrowRelPos={1}
                height={480}
                onNodeClick={((n: any) => {
                  const ticketKinds = ['epic', 'task', 'subtask', 'bug']
                  const id = String(n.id)
                  const isTicket = ticketKinds.some((k) => id.startsWith(`${k}:`))
                  navigate({
                    to: isTicket ? '/tickets/$id' : '/memory/$id',
                    params: { id },
                  })
                }) as any}
              />
              {/* eslint-enable @typescript-eslint/no-explicit-any */}
            </Suspense>
          </div>
        </div>
      )}
    </div>
  )
}

function Legend({ nodes }: { nodes: GraphNode[] }) {
  const kinds = Array.from(new Set(nodes.map((n) => n.kind))).sort()
  if (kinds.length === 0) return null
  return (
    <div className="flex flex-wrap gap-2 rounded-md border border-border/40 bg-background/30 p-2 text-xs">
      <span className="font-mono uppercase tracking-wider text-muted-foreground">Legend:</span>
      {kinds.map((k) => (
        <span key={k} className="inline-flex items-center gap-1">
          <span
            className="inline-block h-2.5 w-2.5 rounded-full"
            style={{ backgroundColor: KIND_COLOR[k] ?? '#94a3b8' }}
          />
          {k}
        </span>
      ))}
    </div>
  )
}

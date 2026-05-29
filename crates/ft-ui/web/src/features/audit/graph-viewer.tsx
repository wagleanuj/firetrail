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
import { lazy, Suspense, useEffect, useMemo, useRef, useState } from 'react'
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

/**
 * The force-graph renders to a `<canvas>`, so it can't consume Tailwind
 * classes — it needs concrete CSS color strings. We resolve them from the
 * design-token CSS custom properties on `:root` so the graph tracks the active
 * theme (dark/light) and stays on the spec palette (§1). Each `--token` holds
 * raw HSL channels (e.g. `187 92% 52%`), so we wrap them in `hsl(...)`.
 */
type Palette = {
  primary: string
  border: string
  muted: string
  kind: Record<string, string>
}

function readVar(styles: CSSStyleDeclaration, name: string, fallback: string): string {
  const raw = styles.getPropertyValue(name).trim()
  return raw ? `hsl(${raw})` : fallback
}

function resolvePalette(): Palette {
  // Spec §1 hsl fallbacks in case computed styles aren't available (SSR/tests).
  if (typeof document === 'undefined') {
    return {
      primary: 'hsl(187 92% 52%)',
      border: 'hsl(215 18% 18%)',
      muted: 'hsl(215 14% 60%)',
      kind: {},
    }
  }
  const s = getComputedStyle(document.documentElement)
  const primary = readVar(s, '--primary', 'hsl(187 92% 52%)')
  const muted = readVar(s, '--muted-foreground', 'hsl(215 14% 60%)')
  const border = readVar(s, '--border', 'hsl(215 18% 18%)')
  // Map record kinds onto semantic / type tokens from the palette.
  const kind: Record<string, string> = {
    epic: readVar(s, '--type-epic', 'hsl(38 92% 60%)'),
    task: readVar(s, '--type-task', 'hsl(255 92% 78%)'),
    subtask: primary,
    bug: readVar(s, '--type-bug', 'hsl(0 75% 64%)'),
    incident: readVar(s, '--danger', 'hsl(0 75% 64%)'),
    finding: readVar(s, '--warning', 'hsl(38 92% 58%)'),
    runbook: readVar(s, '--success', 'hsl(152 60% 50%)'),
    decision: readVar(s, '--info', 'hsl(187 92% 52%)'),
    gotcha: readVar(s, '--warning', 'hsl(38 92% 58%)'),
    memory: muted,
  }
  return { primary, border, muted, kind }
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

  // Resolve palette from CSS tokens once mounted (and keep it stable). The
  // canvas needs literal color strings; see resolvePalette() above.
  const [palette, setPalette] = useState<Palette>(() => resolvePalette())
  useEffect(() => {
    setPalette(resolvePalette())
  }, [])

  const graphData = useMemo(() => {
    if (!data) return { nodes: [], links: [] }
    return {
      nodes: data.nodes.map((n) => ({ ...n, color: palette.kind[n.kind] ?? palette.muted })),
      links: data.edges.map((e) => ({ source: e.from, target: e.to, kind: e.kind })),
    }
  }, [data, palette])

  return (
    <div className="space-y-4">
      <header className="flex flex-wrap items-end gap-3 rounded-[var(--radius)] border border-border bg-surface-2 p-3">
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
        <p className="rounded-[var(--radius)] border border-danger/30 bg-danger/10 px-3 py-2 text-sm text-danger">
          Failed to load graph: {(error as Error).message}
        </p>
      )}

      {data && data.nodes.length === 0 && (
        <p className="rounded-[var(--radius)] border border-dashed border-border px-3 py-8 text-center text-sm text-muted-foreground">
          {data.reason ?? `No relations found from ${id}.`}
        </p>
      )}

      {data && data.nodes.length > 0 && (
        <div className="space-y-2">
          <Legend nodes={data.nodes} palette={palette} />
          <div
            className="overflow-hidden rounded-[var(--radius)] border border-border bg-surface-1 shadow-elevation-1"
            data-testid="force-graph-container"
            style={{ height: 480 }}
          >
            <Suspense fallback={<Skeleton className="h-full w-full" />}>
              {/* eslint-disable @typescript-eslint/no-explicit-any */}
              <ForceGraph2D
                ref={ref as any}
                graphData={graphData as any}
                nodeLabel={((n: any) => `${n.kind}: ${n.title || n.id}`) as any}
                nodeColor={((n: any) => n.color ?? palette.muted) as any}
                linkLabel={((l: any) => l.kind) as any}
                linkColor={() => palette.border}
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

function Legend({ nodes, palette }: { nodes: GraphNode[]; palette: Palette }) {
  const kinds = Array.from(new Set(nodes.map((n) => n.kind))).sort()
  if (kinds.length === 0) return null
  return (
    <div className="flex flex-wrap items-center gap-2.5 rounded-[var(--radius)] border border-border bg-surface-1 px-3 py-2 text-xs">
      <span className="font-mono uppercase tracking-wider text-muted-foreground">Legend</span>
      {kinds.map((k) => (
        <span key={k} className="inline-flex items-center gap-1.5">
          <span
            className="inline-block h-2.5 w-2.5 rounded-full"
            style={{ backgroundColor: palette.kind[k] ?? palette.muted }}
          />
          {k}
        </span>
      ))}
    </div>
  )
}

import { createFileRoute, useNavigate } from '@tanstack/react-router'
import type { GraphDirectionInput } from '@/api/types/GraphDirectionInput'
import { GraphViewer } from '@/features/audit/graph-viewer'

interface GraphSearch {
  id?: string
  direction?: GraphDirectionInput
  depth?: number
}

export const Route = createFileRoute('/audit/graph')({
  component: GraphRoute,
  validateSearch: (s: Record<string, unknown>): GraphSearch => {
    const out: GraphSearch = {}
    if (typeof s.id === 'string') out.id = s.id
    if (s.direction === 'up' || s.direction === 'down' || s.direction === 'both') {
      out.direction = s.direction
    }
    const d = Number(s.depth)
    if (Number.isFinite(d) && d >= 1 && d <= 5) out.depth = Math.floor(d)
    return out
  },
})

function GraphRoute() {
  const search = Route.useSearch()
  const navigate = useNavigate({ from: '/audit/graph' })
  return (
    <div className="mx-auto max-w-6xl space-y-4 p-6">
      <h1 className="font-mono text-lg font-semibold tracking-tight">Graph</h1>
      <GraphViewer
        id={search.id ?? ''}
        direction={search.direction ?? 'both'}
        depth={search.depth ?? 2}
        onChange={(next) => {
          navigate({
            search: (prev) => ({ ...prev, ...next }),
          })
        }}
      />
    </div>
  )
}

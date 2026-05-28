import { createFileRoute, useNavigate } from '@tanstack/react-router'
import { DiffViewer } from '@/features/audit/diff-viewer'

interface DiffSearch {
  base?: string
  head?: string
  scope?: string
  memoryOnly?: boolean
}

export const Route = createFileRoute('/audit/diff')({
  component: DiffRoute,
  validateSearch: (s: Record<string, unknown>): DiffSearch => {
    const out: DiffSearch = {}
    if (typeof s.base === 'string') out.base = s.base
    if (typeof s.head === 'string') out.head = s.head
    if (typeof s.scope === 'string') out.scope = s.scope
    if (s.memoryOnly === true || s.memoryOnly === 'true') out.memoryOnly = true
    return out
  },
})

function DiffRoute() {
  const search = Route.useSearch()
  const navigate = useNavigate({ from: '/audit/diff' })
  return (
    <div className="mx-auto max-w-6xl space-y-4 p-6">
      <h1 className="font-mono text-lg font-semibold tracking-tight">Diff</h1>
      <DiffViewer
        base={search.base ?? 'main'}
        head={search.head ?? 'HEAD'}
        scope={search.scope ?? ''}
        memoryOnly={search.memoryOnly ?? false}
        onChange={(next) => {
          navigate({
            search: (prev) => ({ ...prev, ...next }),
          })
        }}
      />
    </div>
  )
}

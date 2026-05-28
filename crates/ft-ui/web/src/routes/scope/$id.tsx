import { createFileRoute } from '@tanstack/react-router'
import { ScopeExplorer } from '@/features/scope/scope-explorer'

export const Route = createFileRoute('/scope/$id')({
  component: ScopeDetailRoute,
})

function ScopeDetailRoute() {
  const { id } = Route.useParams()
  return <ScopeExplorer selectedId={id} />
}

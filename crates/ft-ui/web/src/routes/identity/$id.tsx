import { createFileRoute } from '@tanstack/react-router'
import { IdentityPanel } from '@/features/identity/identity-panel'

export const Route = createFileRoute('/identity/$id')({
  component: IdentityDetailRoute,
})

function IdentityDetailRoute() {
  const { id } = Route.useParams()
  return <IdentityPanel selectedId={id} />
}

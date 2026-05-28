import { createFileRoute } from '@tanstack/react-router'
import { MemoryDetail } from '@/features/memory/memory-detail'

export const Route = createFileRoute('/memory/$id')({
  component: MemoryDetailRoute,
})

function MemoryDetailRoute() {
  const { id } = Route.useParams()
  return <MemoryDetail id={id} />
}

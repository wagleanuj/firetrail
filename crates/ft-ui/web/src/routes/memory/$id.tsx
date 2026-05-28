import { createFileRoute } from '@tanstack/react-router'
import { MemoryDetail } from '@/features/memory/memory-detail'
import { FeatureErrorBoundary } from '@/components/ui/error-boundary'

export const Route = createFileRoute('/memory/$id')({
  component: MemoryDetailRoute,
})

function MemoryDetailRoute() {
  const { id } = Route.useParams()
  return (
    <FeatureErrorBoundary>
      <MemoryDetail id={id} />
    </FeatureErrorBoundary>
  )
}

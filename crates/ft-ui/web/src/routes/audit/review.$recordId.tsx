import { createFileRoute } from '@tanstack/react-router'
import { ReviewView } from '@/features/audit/review-view'
import { FeatureErrorBoundary } from '@/components/ui/error-boundary'

export const Route = createFileRoute('/audit/review/$recordId')({
  component: ReviewRoute,
})

function ReviewRoute() {
  const { recordId } = Route.useParams()
  return (
    <FeatureErrorBoundary>
      <div className="mx-auto max-w-4xl px-6 py-6">
        <ReviewView recordId={recordId} />
      </div>
    </FeatureErrorBoundary>
  )
}

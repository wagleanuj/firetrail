import { createFileRoute } from '@tanstack/react-router'
import { ReviewView } from '@/features/audit/review-view'

export const Route = createFileRoute('/audit/review/$recordId')({
  component: ReviewRoute,
})

function ReviewRoute() {
  const { recordId } = Route.useParams()
  return (
    <div className="mx-auto max-w-4xl p-6">
      <ReviewView recordId={recordId} />
    </div>
  )
}

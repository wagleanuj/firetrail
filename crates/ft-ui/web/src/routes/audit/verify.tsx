import { createFileRoute } from '@tanstack/react-router'
import { VerifyView } from '@/features/audit/verify-view'
import { FeatureErrorBoundary } from '@/components/ui/error-boundary'

export const Route = createFileRoute('/audit/verify')({
  component: () => (
    <FeatureErrorBoundary>
      <div className="mx-auto max-w-6xl px-6 py-6">
        <VerifyView />
      </div>
    </FeatureErrorBoundary>
  ),
})

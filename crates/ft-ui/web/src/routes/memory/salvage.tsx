import { createFileRoute } from '@tanstack/react-router'
import { SalvageQueue } from '@/features/memory/salvage-queue'
import { FeatureErrorBoundary } from '@/components/ui/error-boundary'

export const Route = createFileRoute('/memory/salvage')({
  component: () => (
    <FeatureErrorBoundary>
      <SalvageQueue />
    </FeatureErrorBoundary>
  ),
})

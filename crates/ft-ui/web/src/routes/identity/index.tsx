import { createFileRoute } from '@tanstack/react-router'
import { IdentityPanel } from '@/features/identity/identity-panel'
import { FeatureErrorBoundary } from '@/components/ui/error-boundary'

export const Route = createFileRoute('/identity/')({
  component: () => (
    <FeatureErrorBoundary>
      <IdentityPanel />
    </FeatureErrorBoundary>
  ),
})

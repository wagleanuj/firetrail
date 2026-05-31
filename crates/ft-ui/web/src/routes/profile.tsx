import { createFileRoute } from '@tanstack/react-router'
import { ProfilePanel } from '@/features/profile/profile-panel'
import { FeatureErrorBoundary } from '@/components/ui/error-boundary'

export const Route = createFileRoute('/profile')({
  component: () => (
    <FeatureErrorBoundary>
      <ProfilePanel />
    </FeatureErrorBoundary>
  ),
})

import { createFileRoute } from '@tanstack/react-router'
import { ScopeExplorer } from '@/features/scope/scope-explorer'
import { FeatureErrorBoundary } from '@/components/ui/error-boundary'

export const Route = createFileRoute('/scope/')({
  component: () => (
    <FeatureErrorBoundary>
      <ScopeExplorer />
    </FeatureErrorBoundary>
  ),
})

import { createFileRoute } from '@tanstack/react-router'
import { LintView } from '@/features/audit/lint-view'
import { FeatureErrorBoundary } from '@/components/ui/error-boundary'

export const Route = createFileRoute('/audit/lint')({
  component: () => (
    <FeatureErrorBoundary>
      <div className="mx-auto max-w-6xl px-6 py-6">
        <LintView />
      </div>
    </FeatureErrorBoundary>
  ),
})

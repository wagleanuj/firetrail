import { createFileRoute } from '@tanstack/react-router'
import { AuditDashboard } from '@/features/audit/dashboard'
import { FeatureErrorBoundary } from '@/components/ui/error-boundary'

export const Route = createFileRoute('/audit/')({
  component: () => (
    <FeatureErrorBoundary>
      <AuditDashboard />
    </FeatureErrorBoundary>
  ),
})

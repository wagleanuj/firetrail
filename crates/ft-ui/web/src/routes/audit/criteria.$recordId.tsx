import { createFileRoute } from '@tanstack/react-router'
import { CriteriaEditor } from '@/features/audit/criteria-editor'
import { FeatureErrorBoundary } from '@/components/ui/error-boundary'
import { PageHeader } from '@/components/page-header'

export const Route = createFileRoute('/audit/criteria/$recordId')({
  component: CriteriaRoute,
})

function CriteriaRoute() {
  const { recordId } = Route.useParams()
  return (
    <FeatureErrorBoundary>
      <div className="mx-auto max-w-3xl space-y-6 px-6 py-6">
        <PageHeader
          title="Criteria"
          subtitle={<span className="font-mono text-xs">{recordId}</span>}
        />
        <CriteriaEditor recordId={recordId} />
      </div>
    </FeatureErrorBoundary>
  )
}

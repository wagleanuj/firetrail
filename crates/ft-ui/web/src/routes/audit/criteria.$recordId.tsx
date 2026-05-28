import { createFileRoute } from '@tanstack/react-router'
import { CriteriaEditor } from '@/features/audit/criteria-editor'

export const Route = createFileRoute('/audit/criteria/$recordId')({
  component: CriteriaRoute,
})

function CriteriaRoute() {
  const { recordId } = Route.useParams()
  return (
    <div className="mx-auto max-w-3xl space-y-4 p-6">
      <header>
        <h1 className="font-mono text-lg font-semibold tracking-tight">Criteria</h1>
        <p className="text-xs text-muted-foreground font-mono">{recordId}</p>
      </header>
      <CriteriaEditor recordId={recordId} />
    </div>
  )
}

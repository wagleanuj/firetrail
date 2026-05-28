import { createFileRoute } from '@tanstack/react-router'
import { LintView } from '@/features/audit/lint-view'

export const Route = createFileRoute('/audit/lint')({
  component: () => (
    <div className="mx-auto max-w-6xl p-6">
      <LintView />
    </div>
  ),
})

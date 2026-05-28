import { createFileRoute } from '@tanstack/react-router'
import { VerifyView } from '@/features/audit/verify-view'

export const Route = createFileRoute('/audit/verify')({
  component: () => (
    <div className="mx-auto max-w-6xl p-6">
      <VerifyView />
    </div>
  ),
})

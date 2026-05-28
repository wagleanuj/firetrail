import { createFileRoute } from '@tanstack/react-router'
import { AuditDashboard } from '@/features/audit/dashboard'

export const Route = createFileRoute('/audit/')({
  component: AuditDashboard,
})

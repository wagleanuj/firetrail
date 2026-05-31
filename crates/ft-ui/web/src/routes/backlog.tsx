import { createFileRoute } from '@tanstack/react-router'
import { Backlog } from '@/features/tickets/backlog'

export const Route = createFileRoute('/backlog')({ component: Backlog })

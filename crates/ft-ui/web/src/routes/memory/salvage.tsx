import { createFileRoute } from '@tanstack/react-router'
import { SalvageQueue } from '@/features/memory/salvage-queue'

export const Route = createFileRoute('/memory/salvage')({
  component: SalvageQueue,
})

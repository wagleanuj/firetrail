import { createFileRoute } from '@tanstack/react-router'
import { EpicsView } from '@/features/epics/epics-view'

export const Route = createFileRoute('/epics/')({ component: EpicsView })

/**
 * Subscribe to `/api/events` and invalidate the memory caches when foreign
 * `memory_*` mutations land. Local mutations are filtered out by `useEvents`'
 * request-id check, so an optimistic update isn't clobbered by its own
 * server echo.
 *
 * Mirrors the W1-C pattern in `features/tickets/use-ticket-events.ts`.
 */
import { useEffect } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import type { Event as MemoryEvent } from '@/api/types/Event'
import { useEvents } from '@/api/hooks/useEvents'
import { memoryShowKey } from './use-memory-query'

export function useMemoryEvents(enabled = true) {
  const queryClient = useQueryClient()
  const { last } = useEvents<MemoryEvent>({ enabled })

  useEffect(() => {
    if (!last) return
    if (!last.kind.startsWith('memory_')) return
    // Any memory mutation invalidates the various list snapshots.
    queryClient.invalidateQueries({ queryKey: ['memory-list'] })
    switch (last.kind) {
      case 'memory_created':
      case 'memory_written':
      case 'memory_salvaged':
        queryClient.invalidateQueries({ queryKey: memoryShowKey(last.id) })
        // Search caches may shift too — invalidate the lot.
        queryClient.invalidateQueries({ queryKey: ['memory-search'] })
        queryClient.invalidateQueries({ queryKey: ['memory-similar'] })
        break
      default:
        break
    }
  }, [last, queryClient])
}

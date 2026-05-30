/**
 * Subscribe to `/api/events` and invalidate the ticket-drawer Docs panel caches
 * when a foreign `doc_edited` mutation lands, so a save on one client refreshes
 * the rendered content + freshness badge on every other (firetrail-e4jv).
 *
 * Local edits are filtered out by `useEvents`' request-id check, so an
 * optimistic update isn't clobbered by its own server echo.
 *
 * Mirrors `features/tickets/use-ticket-events.ts`.
 */
import { useEffect } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import type { Event as AppEvent } from '@/api/types/Event'
import { useEvents } from '@/api/hooks/useEvents'

export function useTicketDocEvents(enabled = true) {
  const queryClient = useQueryClient()
  const { last } = useEvents<AppEvent>({ enabled })

  useEffect(() => {
    if (!last) return
    if (last.kind !== 'doc_edited') return
    // The event carries only the doc id, and a doc may be linked to several
    // work items, so invalidate the whole `['ticket-docs']` key space rather
    // than a single ticket's list.
    queryClient.invalidateQueries({ queryKey: ['ticket-docs'] })
  }, [last, queryClient])
}

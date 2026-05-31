/**
 * Subscribe to `/api/events` once at the app shell level and invalidate the
 * board + per-ticket caches when foreign mutations land. Local mutations are
 * filtered out by `useEvents`' request-id check, so an optimistic update isn't
 * clobbered by its own server echo.
 */
import { useEffect } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import type { Event as TicketEvent } from '@/api/types/Event'
import { useEvents } from '@/api/hooks/useEvents'
import { ticketQueryKey } from './use-ticket-query'

export function useTicketEvents(enabled = true) {
  const queryClient = useQueryClient()
  const { last } = useEvents<TicketEvent>({ enabled })

  useEffect(() => {
    if (!last) return
    // Any ticket-shaped event invalidates the board snapshot and epics roll-up.
    if (last.kind.startsWith('ticket_')) {
      queryClient.invalidateQueries({ queryKey: ['board'] })
      queryClient.invalidateQueries({ queryKey: ['epics'] })
    }
    // Per-ticket invalidations: pull out the ids we know each variant carries.
    switch (last.kind) {
      case 'ticket_created':
      case 'ticket_updated':
      case 'ticket_transitioned':
      case 'ticket_claimed':
      case 'ticket_unclaimed':
      case 'ticket_closed':
        queryClient.invalidateQueries({ queryKey: ticketQueryKey(last.id) })
        break
      case 'ticket_linked':
        queryClient.invalidateQueries({ queryKey: ticketQueryKey(last.from) })
        queryClient.invalidateQueries({ queryKey: ticketQueryKey(last.to) })
        break
      default:
        // memory_written + future variants — ignore for tickets surface.
        break
    }
  }, [last, queryClient])
}

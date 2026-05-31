/**
 * Tests for the ticket SSE hook: ticket_* events invalidate both the board
 * and the epics roll-up query; unrelated events are ignored.
 */
import { describe, it, expect, vi, beforeEach } from 'vitest'
import type { ReactNode } from 'react'
import { renderHook } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import type { Event as AppEvent } from '@/api/types/Event'

// Controllable mock of the SSE hook so no real EventSource opens in jsdom.
let lastEvent: AppEvent | null = null
vi.mock('@/api/hooks/useEvents', () => ({
  useEvents: () => ({ last: lastEvent, state: 'open' }),
}))

import { useTicketEvents } from './use-ticket-events'

function wrapperFor(qc: QueryClient) {
  return function Wrapper({ children }: { children: ReactNode }) {
    return <QueryClientProvider client={qc}>{children}</QueryClientProvider>
  }
}

describe('useTicketEvents', () => {
  let qc: QueryClient

  beforeEach(() => {
    qc = new QueryClient()
    lastEvent = null
  })

  it('invalidates board and epics on ticket_created', () => {
    lastEvent = { kind: 'ticket_created', id: 'task:1' }
    const spy = vi.spyOn(qc, 'invalidateQueries')
    renderHook(() => useTicketEvents(true), { wrapper: wrapperFor(qc) })
    expect(spy).toHaveBeenCalledWith({ queryKey: ['board'] })
    expect(spy).toHaveBeenCalledWith({ queryKey: ['epics'] })
  })

  it('invalidates board and epics on ticket_transitioned', () => {
    lastEvent = { kind: 'ticket_transitioned', id: 'task:2', from: 'open', to: 'closed' }
    const spy = vi.spyOn(qc, 'invalidateQueries')
    renderHook(() => useTicketEvents(true), { wrapper: wrapperFor(qc) })
    expect(spy).toHaveBeenCalledWith({ queryKey: ['board'] })
    expect(spy).toHaveBeenCalledWith({ queryKey: ['epics'] })
  })

  it('invalidates board and epics on ticket_closed', () => {
    lastEvent = { kind: 'ticket_closed', id: 'task:3' }
    const spy = vi.spyOn(qc, 'invalidateQueries')
    renderHook(() => useTicketEvents(true), { wrapper: wrapperFor(qc) })
    expect(spy).toHaveBeenCalledWith({ queryKey: ['board'] })
    expect(spy).toHaveBeenCalledWith({ queryKey: ['epics'] })
  })

  it('ignores unrelated events', () => {
    lastEvent = { kind: 'doc_edited', id: 'doc:1' }
    const spy = vi.spyOn(qc, 'invalidateQueries')
    renderHook(() => useTicketEvents(true), { wrapper: wrapperFor(qc) })
    expect(spy).not.toHaveBeenCalledWith({ queryKey: ['board'] })
    expect(spy).not.toHaveBeenCalledWith({ queryKey: ['epics'] })
  })
})

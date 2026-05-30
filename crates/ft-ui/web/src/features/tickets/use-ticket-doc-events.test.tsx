/**
 * Tests for the doc SSE hook: a `doc_edited` event invalidates the ticket-docs
 * query space; unrelated events are ignored (firetrail-e4jv).
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

import { useTicketDocEvents } from './use-ticket-doc-events'

function wrapperFor(qc: QueryClient) {
  return function Wrapper({ children }: { children: ReactNode }) {
    return <QueryClientProvider client={qc}>{children}</QueryClientProvider>
  }
}

describe('useTicketDocEvents', () => {
  let qc: QueryClient

  beforeEach(() => {
    qc = new QueryClient()
    lastEvent = null
  })

  it('invalidates the ticket-docs query space on doc_edited', () => {
    lastEvent = { kind: 'doc_edited', id: 'doc:1' }
    const spy = vi.spyOn(qc, 'invalidateQueries')
    renderHook(() => useTicketDocEvents(true), { wrapper: wrapperFor(qc) })
    expect(spy).toHaveBeenCalledWith({ queryKey: ['ticket-docs'] })
  })

  it('ignores unrelated events', () => {
    lastEvent = { kind: 'ticket_created', id: 'task:1' }
    const spy = vi.spyOn(qc, 'invalidateQueries')
    renderHook(() => useTicketDocEvents(true), { wrapper: wrapperFor(qc) })
    expect(spy).not.toHaveBeenCalled()
  })
})

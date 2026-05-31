import { render, screen } from '@testing-library/react'
import { describe, it, expect, vi } from 'vitest'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'

// Mock TanStack Router's Link so we don't need a RouterProvider in test scope.
vi.mock('@tanstack/react-router', () => ({
  Link: ({ children, ...rest }: React.PropsWithChildren<Record<string, unknown>>) => (
    <a {...(rest as Record<string, unknown>)}>{children}</a>
  ),
}))

// mock the query hook so the view renders deterministically
vi.mock('./use-epics-query', () => ({
  useEpicsQuery: () => ({
    data: {
      epics: [
        { id: 'E1', short_id: 'EPIC-1', title: 'Auth', status: 'open', priority: 'p1', child_total: 2, child_closed: 2, criteria_total: 0, criteria_met: 0, ready_to_close: true },
        { id: 'E2', short_id: 'EPIC-2', title: 'Billing', status: 'open', priority: 'p2', child_total: 3, child_closed: 1, criteria_total: 0, criteria_met: 0, ready_to_close: false },
      ],
      children: {},
    },
    isLoading: false,
    error: null,
  }),
}))

import { EpicsView } from './epics-view'

function renderEpicsView() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(
    <QueryClientProvider client={qc}>
      <EpicsView />
    </QueryClientProvider>,
  )
}

describe('EpicsView', () => {
  it('shows the close-epic nudge only when ready', () => {
    renderEpicsView()
    expect(screen.getAllByRole('button', { name: /close epic/i })).toHaveLength(1)
  })
})

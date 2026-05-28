import { render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from 'vitest'
import { setupServer } from 'msw/node'
import { http, HttpResponse } from 'msw'

vi.mock('@tanstack/react-router', () => ({
  Link: ({ children, ...rest }: React.PropsWithChildren<Record<string, unknown>>) => (
    <a {...(rest as Record<string, unknown>)}>{children}</a>
  ),
}))

import { DiffViewer } from './diff-viewer'

class FakeES {
  constructor(public url: string) {}
  close() {}
}
;(globalThis as { EventSource?: unknown }).EventSource = FakeES

const server = setupServer(
  http.get('/api/audit/diff', () =>
    HttpResponse.json({
      base: 'main',
      head: 'HEAD',
      memoryOnlyFilter: false,
      scopeFilter: null,
      rows: [
        {
          path: 'memory/notes/abc.md',
          id: 'memory:abc',
          kind: 'memory',
          class: 'memory',
          change: 'added',
          scope: 'core',
          title: 'New note',
        },
      ],
    }),
  ),
)
beforeAll(() => server.listen({ onUnhandledRequest: 'bypass' }))
afterEach(() => server.resetHandlers())
afterAll(() => server.close())

function renderDiff() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(
    <QueryClientProvider client={qc}>
      <DiffViewer base="main" head="HEAD" scope="" memoryOnly={false} onChange={() => {}} />
    </QueryClientProvider>,
  )
}

describe('<DiffViewer />', () => {
  it('renders diff rows from the API', async () => {
    renderDiff()
    await waitFor(() => {
      expect(screen.getByTestId('diff-rows')).toBeInTheDocument()
    })
    expect(screen.getByText('memory/notes/abc.md')).toBeInTheDocument()
    expect(screen.getByText('New note')).toBeInTheDocument()
  })
})

vi.spyOn(console, 'error').mockImplementation(() => {})

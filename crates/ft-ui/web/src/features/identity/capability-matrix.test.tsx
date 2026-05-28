import { render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from 'vitest'
import { setupServer } from 'msw/node'
import { http, HttpResponse } from 'msw'
import { CapabilityMatrix } from './capability-matrix'

class FakeES {
  constructor(public url: string) {}
  close() {}
}
;(globalThis as { EventSource?: unknown }).EventSource = FakeES

const server = setupServer(
  http.get('/api/identity/alice/capabilities', () =>
    HttpResponse.json({
      identity: 'alice',
      kind: 'human',
      status: 'active',
      capabilities: [
        { capability: 'can_promote_verified', granted: true, overridden: true },
        { capability: 'can_redact', granted: false, overridden: false },
      ],
    }),
  ),
)
beforeAll(() => server.listen({ onUnhandledRequest: 'bypass' }))
afterEach(() => server.resetHandlers())
afterAll(() => server.close())

function renderMatrix() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(
    <QueryClientProvider client={qc}>
      <CapabilityMatrix identity="alice" />
    </QueryClientProvider>,
  )
}

describe('<CapabilityMatrix />', () => {
  it('renders the capability rows with override indicators', async () => {
    renderMatrix()
    await waitFor(() => {
      expect(screen.getByTestId('capability-matrix')).toBeInTheDocument()
    })
    expect(screen.getByText('can_promote_verified')).toBeInTheDocument()
    expect(screen.getByText('can_redact')).toBeInTheDocument()
    expect(screen.getByTestId('override-can_promote_verified')).toBeInTheDocument()
  })
})

vi.spyOn(console, 'error').mockImplementation(() => {})

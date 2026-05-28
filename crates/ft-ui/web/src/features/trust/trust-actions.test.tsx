import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from 'vitest'
import { setupServer } from 'msw/node'
import { http, HttpResponse } from 'msw'
import { TrustActions } from './trust-actions'

class FakeES {
  constructor(public url: string) {}
  close() {}
}
;(globalThis as { EventSource?: unknown }).EventSource = FakeES

let promoteBody: unknown = null

const server = setupServer(
  http.post('/api/trust/memory:abc/promote', async ({ request }) => {
    promoteBody = await request.json()
    return HttpResponse.json({ record: { envelope: { id: 'memory:abc' }, body: {} } })
  }),
)
beforeAll(() => server.listen({ onUnhandledRequest: 'bypass' }))
afterEach(() => {
  server.resetHandlers()
  promoteBody = null
})
afterAll(() => server.close())

function renderActions(state: string, risk: string | null = null) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(
    <QueryClientProvider client={qc}>
      <TrustActions recordId="memory:abc" trustState={state} riskClass={risk} />
    </QueryClientProvider>,
  )
}

describe('<TrustActions />', () => {
  it('renders only valid transitions for the source state', () => {
    renderActions('reviewed')
    expect(screen.getByTestId('trust-op-promote')).toBeInTheDocument()
    expect(screen.getByTestId('trust-op-deprecate')).toBeInTheDocument()
    expect(screen.queryByTestId('trust-op-review')).toBeNull()
  })

  it('blocks promote without evidence on high-stakes risk classes', async () => {
    renderActions('reviewed', 'security')
    fireEvent.click(screen.getByTestId('trust-op-promote'))
    const confirm = await screen.findByTestId('promote-confirm')
    expect(confirm).toBeDisabled()
  })

  it('hides every action when the state is terminal (redacted)', () => {
    renderActions('redacted')
    expect(screen.queryByTestId('trust-op-promote')).toBeNull()
    expect(screen.queryByTestId('trust-op-redact')).toBeNull()
    expect(screen.getByText(/terminal/i)).toBeInTheDocument()
  })

  it('submits the promote payload with evidence', async () => {
    renderActions('reviewed', null)
    fireEvent.click(screen.getByTestId('trust-op-promote'))
    fireEvent.change(screen.getByPlaceholderText('https://…'), {
      target: { value: 'https://example.com/pr/1' },
    })
    fireEvent.click(screen.getByTestId('promote-confirm'))
    await waitFor(() => {
      expect(promoteBody).toMatchObject({
        evidenceUrl: 'https://example.com/pr/1',
        evidenceType: 'pull_request',
      })
    })
  })
})

vi.spyOn(console, 'error').mockImplementation(() => {})

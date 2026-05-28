import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from 'vitest'
import { setupServer } from 'msw/node'
import { http, HttpResponse } from 'msw'
import { RegisterIdentityDialog } from './register-dialog'

class FakeES {
  constructor(public url: string) {}
  close() {}
}
;(globalThis as { EventSource?: unknown }).EventSource = FakeES

let lastBody: unknown = null

const server = setupServer(
  http.post('/api/identity', async ({ request }) => {
    lastBody = await request.json()
    return HttpResponse.json({
      identity: {
        id: 'alice',
        name: 'Alice',
        kind: 'human',
        status: 'active',
        emails: ['alice@example.com'],
        machines: [],
        capabilities: [],
      },
    })
  }),
)
beforeAll(() => server.listen({ onUnhandledRequest: 'bypass' }))
afterEach(() => {
  server.resetHandlers()
  lastBody = null
})
afterAll(() => server.close())

function renderDialog() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const onChange = vi.fn()
  const utils = render(
    <QueryClientProvider client={qc}>
      <RegisterIdentityDialog open onOpenChange={onChange} />
    </QueryClientProvider>,
  )
  return { ...utils, onChange }
}

describe('<RegisterIdentityDialog />', () => {
  it('validates email and submits the parsed body', async () => {
    const { onChange } = renderDialog()

    fireEvent.change(screen.getByPlaceholderText(/alice \/ bot-claude/i), {
      target: { value: 'alice' },
    })
    fireEvent.change(screen.getByPlaceholderText(/Alice Example/i), {
      target: { value: 'Alice' },
    })
    fireEvent.change(screen.getByPlaceholderText('alice@example.com'), {
      target: { value: 'not-an-email' },
    })
    fireEvent.click(screen.getByRole('button', { name: /^Register$/i }))
    await waitFor(() => {
      expect(screen.getByText(/must be a valid email/i)).toBeInTheDocument()
    })

    fireEvent.change(screen.getByPlaceholderText('alice@example.com'), {
      target: { value: 'alice@example.com' },
    })
    fireEvent.change(screen.getByPlaceholderText(/can_promote_verified/), {
      target: { value: 'can_redact=false' },
    })
    fireEvent.click(screen.getByRole('button', { name: /^Register$/i }))

    await waitFor(() => {
      expect(lastBody).toMatchObject({
        id: 'alice',
        emails: ['alice@example.com'],
        kind: 'human',
        capabilities: [{ key: 'can_redact', value: false }],
      })
      expect(onChange).toHaveBeenCalledWith(false)
    })
  })
})

vi.spyOn(console, 'error').mockImplementation(() => {})

import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from 'vitest'
import { setupServer } from 'msw/node'
import { http, HttpResponse } from 'msw'
import { CriteriaEditor } from './criteria-editor'

class FakeES {
  constructor(public url: string) {}
  close() {}
}
;(globalThis as { EventSource?: unknown }).EventSource = FakeES

let lastPatch: { checked?: boolean } = {}

const initialItems = [
  { index: 1, id: 'ac-01', text: 'first', checked: false, evidenceUrl: null },
  { index: 2, id: 'ac-02', text: 'second', checked: true, evidenceUrl: null },
]

const server = setupServer(
  http.get('/api/audit/criteria/task:abc', () =>
    HttpResponse.json({ recordId: 'task:abc', items: initialItems }),
  ),
  http.patch('/api/audit/criteria/task:abc/ac-01', async ({ request }) => {
    lastPatch = (await request.json()) as { checked?: boolean }
    return HttpResponse.json({
      recordId: 'task:abc',
      items: [
        { ...initialItems[0], checked: lastPatch.checked ?? false },
        initialItems[1],
      ],
    })
  }),
)
beforeAll(() => server.listen({ onUnhandledRequest: 'bypass' }))
afterEach(() => {
  server.resetHandlers()
  lastPatch = {}
})
afterAll(() => server.close())

function renderEditor() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(
    <QueryClientProvider client={qc}>
      <CriteriaEditor recordId="task:abc" />
    </QueryClientProvider>,
  )
}

describe('<CriteriaEditor />', () => {
  it('toggles a criterion optimistically and confirms with the server', async () => {
    renderEditor()
    await waitFor(() => {
      expect(screen.getByText('first')).toBeInTheDocument()
    })
    fireEvent.click(screen.getByTestId('criterion-ac-01'))
    await waitFor(() => {
      expect(lastPatch.checked).toBe(true)
    })
  })
})

vi.spyOn(console, 'error').mockImplementation(() => {})

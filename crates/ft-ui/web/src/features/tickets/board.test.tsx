import { render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  RouterProvider,
} from '@tanstack/react-router'
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from 'vitest'
import { setupServer } from 'msw/node'
import { http, HttpResponse } from 'msw'
import { Board } from './board'

const board = {
  todo: [
    { id: 'task:aaaa1111bbbb', short_id: 'task:aaaa', title: 'Wire kanban', priority: 'p1', owner: null },
  ],
  in_progress: [
    { id: 'task:cccc2222dddd', short_id: 'task:cccc', title: 'Add SSE filter', priority: 'p2', owner: 'anuj' },
  ],
  review: [],
  done: [],
}

const server = setupServer(
  http.get('/api/tickets/board', () => HttpResponse.json(board)),
)

beforeAll(() => server.listen({ onUnhandledRequest: 'bypass' }))
afterEach(() => server.resetHandlers())
afterAll(() => server.close())

// jsdom does not implement EventSource — stub it so the useEvents hook
// mounted by AppShell doesn't blow up. The Board itself doesn't need it
// because we render the component directly here.
class FakeES {
  url: string
  onmessage: ((ev: MessageEvent) => void) | null = null
  onopen: (() => void) | null = null
  onerror: (() => void) | null = null
  constructor(url: string) {
    this.url = url
  }
  close() {}
}
;(globalThis as { EventSource?: unknown }).EventSource = FakeES

function renderBoard() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const rootRoute = createRootRoute({ component: () => <Board onCreateClick={() => undefined} /> })
  const indexRoute = createRoute({ getParentRoute: () => rootRoute, path: '/' })
  const router = createRouter({
    routeTree: rootRoute.addChildren([indexRoute]),
    history: createMemoryHistory({ initialEntries: ['/'] }),
  })
  return render(
    <QueryClientProvider client={qc}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  )
}

describe('<Board />', () => {
  it('renders four columns and pulls cards from the API', async () => {
    renderBoard()
    await waitFor(() => {
      expect(screen.getByText('Wire kanban')).toBeInTheDocument()
    })
    expect(screen.getByTestId('column-todo')).toBeInTheDocument()
    expect(screen.getByTestId('column-in_progress')).toBeInTheDocument()
    expect(screen.getByTestId('column-review')).toBeInTheDocument()
    expect(screen.getByTestId('column-done')).toBeInTheDocument()
    expect(screen.getByText('Add SSE filter')).toBeInTheDocument()
  })

  it('shows the empty state when the board is empty', async () => {
    server.use(
      http.get('/api/tickets/board', () =>
        HttpResponse.json({ todo: [], in_progress: [], review: [], done: [] }),
      ),
    )
    renderBoard()
    expect(await screen.findByText(/no tickets yet/i)).toBeInTheDocument()
  })
})

// Silence "act" warnings from async tanstack-query state updates that race
// the test cleanup.
vi.spyOn(console, 'error').mockImplementation(() => {})

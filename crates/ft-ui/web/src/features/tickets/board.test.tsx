import { render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  RouterProvider,
} from '@tanstack/react-router'
import { afterAll, afterEach, beforeAll, describe, expect, it } from 'vitest'
import { setupServer } from 'msw/node'
import { http, HttpResponse } from 'msw'
import { Board } from './board'

const board = {
  todo: [
    {
      id: 'TASK-aaaa1111bbbb2222',
      short_id: 'TASK-aaaa1111',
      title: 'Wire kanban',
      kind: 'task',
      priority: 'p1',
      owner: null,
      epic_id: null,
      criteria_total: 0,
      criteria_met: 0,
      subtask_count: 0,
      blocked_by_count: 0,
    },
  ],
  in_progress: [
    {
      id: 'TASK-cccc2222dddd3333',
      short_id: 'TASK-cccc2222',
      title: 'Add SSE filter',
      kind: 'task',
      priority: 'p2',
      owner: 'anuj',
      epic_id: null,
      criteria_total: 0,
      criteria_met: 0,
      subtask_count: 0,
      blocked_by_count: 0,
    },
  ],
  review: [],
  done: [],
  epics: [],
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

async function renderBoard() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const rootRoute = createRootRoute({ component: () => <Board onCreateClick={() => undefined} /> })
  const indexRoute = createRoute({ getParentRoute: () => rootRoute, path: '/' })
  const router = createRouter({
    routeTree: rootRoute.addChildren([indexRoute]),
    history: createMemoryHistory({ initialEntries: ['/'] }),
  })
  await router.load()
  return render(
    <QueryClientProvider client={qc}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  )
}

describe('<Board />', () => {
  it('renders four columns and pulls cards from the API', async () => {
    await renderBoard()
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
        HttpResponse.json({ todo: [], in_progress: [], review: [], done: [], epics: [] }),
      ),
    )
    await renderBoard()
    expect(await screen.findByText(/no tickets yet/i)).toBeInTheDocument()
  })
})

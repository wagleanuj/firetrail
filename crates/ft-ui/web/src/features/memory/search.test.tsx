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
import { MemorySearch } from './memory-search'

const server = setupServer(
  http.get('/api/memory/search', () =>
    HttpResponse.json({
      mode: 'lexical',
      hits: [
        {
          id: 'memory:aaaa1111',
          kind: 'memory',
          title: 'Hello world',
          score: 0.42,
          trust: 'reviewed',
          mode: 'lexical',
          quarantine: false,
        },
      ],
      warnings: ['embedder unavailable; falling back to lexical'],
    }),
  ),
)

beforeAll(() => server.listen({ onUnhandledRequest: 'bypass' }))
afterEach(() => server.resetHandlers())
afterAll(() => server.close())

class FakeES {
  url: string
  constructor(url: string) {
    this.url = url
  }
  close() {}
}
;(globalThis as { EventSource?: unknown }).EventSource = FakeES

function renderSearch(initialPath: string) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const rootRoute = createRootRoute()
  const memoryIdRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/memory/$id',
    component: () => <div>memory detail</div>,
  })
  const ticketIdRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/tickets/$id',
    component: () => <div>ticket detail</div>,
  })
  const scopeIdRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/scope/$id',
    component: () => <div>scope detail</div>,
  })
  const searchRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/memory/search',
    component: MemorySearch,
    validateSearch: (s: Record<string, unknown>) => s as Record<string, unknown>,
  })
  const router = createRouter({
    routeTree: rootRoute.addChildren([
      searchRoute,
      memoryIdRoute,
      ticketIdRoute,
      scopeIdRoute,
    ]),
    history: createMemoryHistory({ initialEntries: [initialPath] }),
  })
  return render(
    <QueryClientProvider client={qc}>
      {/* eslint-disable-next-line @typescript-eslint/no-explicit-any */}
      <RouterProvider router={router as any} />
    </QueryClientProvider>,
  )
}

describe('<MemorySearch />', () => {
  it('renders ranked hits from the API', async () => {
    renderSearch('/memory/search?q=hello')
    await waitFor(() => {
      expect(screen.getByText('Hello world')).toBeInTheDocument()
    })
    expect(screen.getByTestId('search-results')).toBeInTheDocument()
  })

  it('surfaces non-fatal warnings as an amber banner', async () => {
    renderSearch('/memory/search?q=hello')
    await waitFor(() => {
      expect(screen.getByTestId('search-warnings')).toBeInTheDocument()
    })
    expect(
      screen.getByText(/embedder unavailable; falling back to lexical/i),
    ).toBeInTheDocument()
  })

  it('links an audit synthetic hit to its underlying record, not /memory/<audit-id> (firetrail-g5n6)', async () => {
    const task = `TASK-${'a'.repeat(64)}`
    server.use(
      http.get('/api/memory/search', () =>
        HttpResponse.json({
          mode: 'lexical',
          hits: [
            {
              id: `audit:${task}#h0`,
              kind: 'audit',
              title: 'task created: Fix login',
              score: 0.9,
              trust: 'reviewed',
              mode: 'lexical',
              quarantine: false,
            },
          ],
          warnings: [],
        }),
      ),
    )
    renderSearch('/memory/search?q=audit')
    const link = await screen.findByRole('link', { name: /task created: Fix login/i })
    // The audit echo resolves to its task on the tickets surface — never a
    // dead `/memory/audit:…` link (the reported "memory not found" 404).
    expect(link).toHaveAttribute('href', `/tickets/${task}`)
  })
})

vi.spyOn(console, 'error').mockImplementation(() => {})

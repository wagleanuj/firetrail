import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from '@tanstack/react-router'
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from 'vitest'
import { setupServer } from 'msw/node'
import { http, HttpResponse } from 'msw'
import { CommandPalette } from './command-palette'

let lastRequestUrl = ''

const server = setupServer(
  http.get('/api/search', ({ request }) => {
    lastRequestUrl = request.url
    return HttpResponse.json({
      mode: 'lexical',
      hits: [
        {
          id: 'TASK-1111',
          kind: 'task',
          title: 'Fix the frobnicator',
          score: 0.91,
          trust: 'reviewed',
          scope: 'apps/checkout',
          mode: 'lexical',
          quarantine: false,
        },
        {
          id: 'GOTCHA-2222',
          kind: 'gotcha',
          title: 'frobnicator wedges under load',
          score: 0.42,
          trust: 'draft',
          scope: null,
          mode: 'lexical',
          quarantine: false,
        },
      ],
      warnings: [],
    })
  }),
)

beforeAll(() => server.listen({ onUnhandledRequest: 'bypass' }))
afterEach(() => {
  server.resetHandlers()
  lastRequestUrl = ''
})
afterAll(() => server.close())

class FakeES {
  url: string
  constructor(url: string) {
    this.url = url
  }
  close() {}
}
;(globalThis as { EventSource?: unknown }).EventSource = FakeES

// cmdk (and Radix) reach for ResizeObserver, which jsdom does not implement.
class FakeResizeObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}
;(globalThis as { ResizeObserver?: unknown }).ResizeObserver = FakeResizeObserver
// jsdom lacks scrollIntoView, which cmdk calls when the selection moves.
if (!HTMLElement.prototype.scrollIntoView) {
  HTMLElement.prototype.scrollIntoView = () => {}
}

const navSpy = vi.fn()

function renderPalette() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const rootRoute = createRootRoute({
    component: () => (
      <>
        <CommandPalette open onOpenChange={() => {}} />
        <Outlet />
      </>
    ),
  })
  const noop = () => <div />
  const router = createRouter({
    routeTree: rootRoute.addChildren([
      createRoute({ getParentRoute: () => rootRoute, path: '/', component: noop }),
      createRoute({ getParentRoute: () => rootRoute, path: '/tickets/$id', component: noop }),
      createRoute({ getParentRoute: () => rootRoute, path: '/memory/$id', component: noop }),
      createRoute({ getParentRoute: () => rootRoute, path: '/memory', component: noop }),
      createRoute({ getParentRoute: () => rootRoute, path: '/scope', component: noop }),
      createRoute({ getParentRoute: () => rootRoute, path: '/identity', component: noop }),
      createRoute({ getParentRoute: () => rootRoute, path: '/audit', component: noop }),
    ]),
    history: createMemoryHistory({ initialEntries: ['/'] }),
  })
  router.navigate = navSpy
  return render(
    <QueryClientProvider client={qc}>
      {/* eslint-disable-next-line @typescript-eslint/no-explicit-any */}
      <RouterProvider router={router as any} />
    </QueryClientProvider>,
  )
}

async function typeQuery(text: string) {
  // The palette mounts asynchronously through the router, so wait for the
  // input before driving it.
  const input = await screen.findByPlaceholderText(/search tasks/i)
  fireEvent.change(input, { target: { value: text } })
}

describe('<CommandPalette />', () => {
  it('shows live cross-domain results with kind + trust badges', async () => {
    renderPalette()

    // Static nav is present before typing (router mounts the palette async).
    expect(await screen.findByText('Board')).toBeInTheDocument()

    await typeQuery('frobnicator')

    await waitFor(() => {
      expect(screen.getByText('Fix the frobnicator')).toBeInTheDocument()
    })
    // Both kinds render in the results list.
    const results = within(screen.getByTestId('palette-results'))
    expect(results.getByText('frobnicator wedges under load')).toBeInTheDocument()
    // Kind badges (task + gotcha) and a trust badge, scoped to results so the
    // filter chips of the same name don't collide.
    expect(results.getByText('task')).toBeInTheDocument()
    expect(results.getByText('gotcha')).toBeInTheDocument()
    expect(results.getByText('reviewed')).toBeInTheDocument()
  })

  it('navigates to the record route when a result is selected', async () => {
    renderPalette()
    navSpy.mockClear()

    await typeQuery('frobnicator')
    const hit = await screen.findByText('Fix the frobnicator')
    fireEvent.click(hit)

    await waitFor(() => {
      expect(navSpy).toHaveBeenCalledWith(
        expect.objectContaining({ to: '/tickets/$id', params: { id: 'TASK-1111' } }),
      )
    })
  })

  it('passes the kind filter to the backend when a chip is toggled', async () => {
    renderPalette()

    await typeQuery('frobnicator')
    await screen.findByText('Fix the frobnicator')

    fireEvent.click(screen.getByTestId('kind-chip-gotcha'))

    await waitFor(() => {
      expect(lastRequestUrl).toContain('kind=gotcha')
    })
  })

  it('passes the selected search mode to the backend', async () => {
    renderPalette()

    await typeQuery('frobnicator')
    await screen.findByText('Fix the frobnicator')

    fireEvent.click(screen.getByTestId('mode-segment-vector'))

    await waitFor(() => {
      expect(lastRequestUrl).toContain('mode=vector')
    })
  })
})

vi.spyOn(console, 'error').mockImplementation(() => {})

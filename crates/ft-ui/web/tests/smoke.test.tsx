import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  RouterProvider,
} from '@tanstack/react-router'
import { AppShell } from '@/components/app-shell'

// Stub EventSource so AppShell's SSE subscription doesn't crash jsdom.
class FakeES {
  url: string
  constructor(url: string) {
    this.url = url
  }
  close() {}
}
;(globalThis as { EventSource?: unknown }).EventSource = FakeES

describe('smoke', () => {
  it('renders the app shell wordmark', async () => {
    const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    const rootRoute = createRootRoute({ component: AppShell })
    const indexRoute = createRoute({
      getParentRoute: () => rootRoute,
      path: '/',
      component: () => <div>placeholder</div>,
    })
    const router = createRouter({
      routeTree: rootRoute.addChildren([indexRoute]),
      history: createMemoryHistory({ initialEntries: ['/'] }),
    })
    render(
      <QueryClientProvider client={qc}>
        <RouterProvider router={router} />
      </QueryClientProvider>,
    )
    expect(await screen.findByText(/firetrail/i)).toBeInTheDocument()
    expect(await screen.findByText('Board')).toBeInTheDocument()
  })
})

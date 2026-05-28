import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  RouterProvider,
} from '@tanstack/react-router'
import { Route as HomeRoute } from '@/routes/index'
import { AppShell } from '@/components/app-shell'

describe('smoke', () => {
  it('renders the Wave 0 banner on the home route', async () => {
    const rootRoute = createRootRoute({ component: AppShell })
    const indexRoute = createRoute({
      getParentRoute: () => rootRoute,
      path: '/',
      component: HomeRoute.options.component,
    })
    const router = createRouter({
      routeTree: rootRoute.addChildren([indexRoute]),
      history: createMemoryHistory({ initialEntries: ['/'] }),
    })
    render(<RouterProvider router={router} />)
    expect(await screen.findByText(/Wave 0 ready/i)).toBeInTheDocument()
    expect(await screen.findByText(/Firetrail/)).toBeInTheDocument()
  })
})

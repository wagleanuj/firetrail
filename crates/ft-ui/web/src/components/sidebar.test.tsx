import { render, screen } from '@testing-library/react'
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from '@tanstack/react-router'
import { describe, expect, it } from 'vitest'
import { Sidebar } from './sidebar'

function renderSidebar() {
  const rootRoute = createRootRoute({
    component: () => (
      <>
        <Sidebar />
        <Outlet />
      </>
    ),
  })
  const noop = () => <div />
  const router = createRouter({
    routeTree: rootRoute.addChildren([
      createRoute({ getParentRoute: () => rootRoute, path: '/', component: noop }),
      createRoute({ getParentRoute: () => rootRoute, path: '/memory', component: noop }),
      createRoute({ getParentRoute: () => rootRoute, path: '/memory/search', component: noop }),
    ]),
    history: createMemoryHistory({ initialEntries: ['/'] }),
  })
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  return render(<RouterProvider router={router as any} />)
}

describe('<Sidebar />', () => {
  it('exposes a Search link to the memory search page', async () => {
    renderSidebar()
    const link = await screen.findByRole('link', { name: /search/i })
    expect(link).toHaveAttribute('href', '/memory/search')
  })
})

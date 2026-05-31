import { render, screen, waitFor } from '@testing-library/react'
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  RouterProvider,
} from '@tanstack/react-router'
import { describe, it, expect } from 'vitest'
import { BoardCardBody } from './board-card'
import type { BoardCard } from '@/api/types/BoardCard'

const card: BoardCard = {
  id: 'TASK-1',
  short_id: 'TASK-1',
  title: 'Reset flow',
  kind: 'task',
  priority: 'p2',
  owner: 'alice',
  epic_id: 'EPIC-9',
  criteria_total: 5,
  criteria_met: 3,
  subtask_count: 2,
  blocked_by_count: 1,
}

async function renderWithRouter(ui: React.ReactElement) {
  const rootRoute = createRootRoute({ component: () => ui })
  const indexRoute = createRoute({ getParentRoute: () => rootRoute, path: '/' })
  const router = createRouter({
    routeTree: rootRoute.addChildren([indexRoute]),
    history: createMemoryHistory({ initialEntries: ['/'] }),
  })
  await router.load()
  return render(<RouterProvider router={router} />)
}

describe('BoardCardBody', () => {
  it('renders the type pill from kind', async () => {
    await renderWithRouter(<BoardCardBody card={card} epicTitle="Ship v1" />)
    // The badge renders the kind text; getAllByText handles the short_id also matching
    await waitFor(() => expect(screen.getAllByText(/task/i).length).toBeGreaterThan(0))
    // Specifically check the badge element has the kind label
    const badges = screen.getAllByText(/^task$/i)
    expect(badges.length).toBeGreaterThan(0)
  })

  it('renders criteria progress and blocked badge', async () => {
    await renderWithRouter(<BoardCardBody card={card} epicTitle="Ship v1" />)
    await waitFor(() => expect(screen.getByText('3/5')).toBeInTheDocument())
    expect(screen.getByText(/blocked/i)).toBeInTheDocument()
  })

  it('renders the card title', async () => {
    await renderWithRouter(<BoardCardBody card={card} epicTitle="Ship v1" />)
    await waitFor(() => expect(screen.getByText('Reset flow')).toBeInTheDocument())
  })

  it('renders the epic chip when epicTitle is provided', async () => {
    await renderWithRouter(<BoardCardBody card={card} epicTitle="Ship v1" />)
    await waitFor(() => expect(screen.getByText('Ship v1')).toBeInTheDocument())
  })

  it('renders subtask count', async () => {
    await renderWithRouter(<BoardCardBody card={card} epicTitle="Ship v1" />)
    await waitFor(() => expect(screen.getByText(/⛬ 2/)).toBeInTheDocument())
  })

  it('renders owner', async () => {
    await renderWithRouter(<BoardCardBody card={card} epicTitle="Ship v1" />)
    await waitFor(() => expect(screen.getByText('alice')).toBeInTheDocument())
  })
})

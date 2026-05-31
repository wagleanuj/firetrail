/**
 * Unit tests for the ticket-detail relation filtering (firetrail-e4jv).
 * Extended: epic breadcrumb + typed children (firetrail-6no5.12).
 */
import { render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  RouterProvider,
} from '@tanstack/react-router'
import { afterAll, afterEach, beforeAll, describe, it, expect, vi } from 'vitest'
import { setupServer } from 'msw/node'
import { http, HttpResponse } from 'msw'
import type { Relation } from '@/api/wire/relation'
import type { TicketRelationKind } from '@/api/types/TicketRelationKind'
import type { ShowOutputWire, RecordKindWire } from '@/api/wire/record'
import { visibleRelations, TicketDetail } from './ticket-detail'

// Mock heavy editor components that break in jsdom.
vi.mock('@/components/markdown-editor', () => ({
  MarkdownEditor: ({ value }: { value: string }) => <div data-testid="md-view">{value}</div>,
  useMarkdownEditor: () => null,
}))

vi.mock('./description-editor', () => ({
  DescriptionEditor: ({ value }: { value: string }) => <div>{value}</div>,
}))

vi.mock('@/features/audit/criteria-editor', () => ({
  CriteriaEditor: () => null,
}))

vi.mock('./docs-panel', () => ({
  DocsPanel: () => null,
}))

function rel(kind: string): Relation {
  return {
    from: 'task:a',
    to: 'task:b',
    kind: kind as TicketRelationKind,
    created_at: '2026-05-30T00:00:00Z',
    created_by: { id: 'id:1', name: 'tester' },
  }
}

describe('visibleRelations', () => {
  it('hides documented-in edges (they live in the Docs panel)', () => {
    const input = [rel('blocks'), rel('documented-in'), rel('related-to')]
    const out = visibleRelations(input)
    expect(out.map((r) => r.kind)).toEqual(['blocks', 'related-to'])
  })

  it('passes through when there are no doc edges', () => {
    const input = [rel('blocks'), rel('child-of')]
    expect(visibleRelations(input)).toHaveLength(2)
  })

  it('returns empty when every relation is a doc edge', () => {
    expect(visibleRelations([rel('documented-in')])).toEqual([])
  })
})

// ──────────────────────────────────────────────────────────────
// Rendering tests — epic breadcrumb + typed children
// ──────────────────────────────────────────────────────────────

const TASK_ID = 'task:aaaa1111bbbb2222'
const EPIC_ID = 'epic:cccc3333dddd4444'
const CHILD_ID = 'subtask:eeee5555ffff6666'

const makeEnvelope = (id: string, kind: RecordKindWire, title: string) => ({
  id,
  kind,
  title,
  status: 'open' as const,
  priority: 'p2' as const,
  owner: null,
  created_by: { id: 'user:1', name: 'tester' },
  created_at: '2026-05-30T00:00:00Z',
  updated_at: '2026-05-30T00:00:00Z',
  closed_at: null,
  owning_scope: null,
  affected_scopes: [],
  applies_to: [],
  labels: [],
})

const taskPayload: ShowOutputWire = {
  record: {
    envelope: makeEnvelope(TASK_ID, 'task', 'Add auth flow'),
    body: { kind: 'task', description: '', claim: null },
  },
  relations: [
    // task is child-of the epic (outbound: from=TASK_ID, to=EPIC_ID)
    {
      from: TASK_ID,
      to: EPIC_ID,
      kind: 'child-of',
      created_at: '2026-05-30T00:00:00Z',
      created_by: { id: 'user:1', name: 'tester' },
    },
    // task is parent-of the subtask (outbound: from=TASK_ID, to=CHILD_ID)
    {
      from: TASK_ID,
      to: CHILD_ID,
      kind: 'parent-of',
      created_at: '2026-05-30T00:00:00Z',
      created_by: { id: 'user:1', name: 'tester' },
    },
  ],
}

const epicPayload: ShowOutputWire = {
  record: {
    envelope: makeEnvelope(EPIC_ID, 'epic', 'Ship v1 auth'),
    body: { kind: 'epic', description: '' },
  },
  relations: [],
}

const childPayload: ShowOutputWire = {
  record: {
    envelope: makeEnvelope(CHILD_ID, 'subtask', 'Migrate schema'),
    body: { kind: 'subtask', description: '', claim: null },
  },
  relations: [],
}

const server = setupServer(
  http.get(`/api/tickets/${TASK_ID}`, () => HttpResponse.json(taskPayload)),
  http.get(`/api/tickets/${EPIC_ID}`, () => HttpResponse.json(epicPayload)),
  http.get(`/api/tickets/${CHILD_ID}`, () => HttpResponse.json(childPayload)),
  // Stub out audit criteria
  http.get(`/api/tickets/${TASK_ID}/audit/criteria`, () => HttpResponse.json({ items: [] })),
)

beforeAll(() => server.listen({ onUnhandledRequest: 'bypass' }))
afterEach(() => server.resetHandlers())
afterAll(() => server.close())

async function renderTicketDetail(id: string) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } })

  const rootRoute = createRootRoute({
    component: () => (
      <QueryClientProvider client={qc}>
        <TicketDetail id={id} />
      </QueryClientProvider>
    ),
  })
  const indexRoute = createRoute({ getParentRoute: () => rootRoute, path: '/' })
  const ticketsRoute = createRoute({ getParentRoute: () => rootRoute, path: '/tickets/$id' })
  const router = createRouter({
    routeTree: rootRoute.addChildren([indexRoute, ticketsRoute]),
    history: createMemoryHistory({ initialEntries: ['/'] }),
  })
  await router.load()
  return render(<RouterProvider router={router} />)
}

describe('<TicketDetail /> — epic breadcrumb + typed children', () => {
  it('shows the epic breadcrumb with the epic title', async () => {
    await renderTicketDetail(TASK_ID)
    expect(await screen.findByText('Ship v1 auth')).toBeInTheDocument()
  })

  it('shows the child ticket by its title (not raw id)', async () => {
    await renderTicketDetail(TASK_ID)
    expect(await screen.findByText('Migrate schema')).toBeInTheDocument()
  })

  it('shows a type pill for the child ticket kind', async () => {
    await renderTicketDetail(TASK_ID)
    // Wait for the child to load
    await screen.findByText('Migrate schema')
    // Should show a 'subtask' type pill somewhere in the children section
    await waitFor(() => {
      const pills = screen.getAllByText(/subtask/i)
      expect(pills.length).toBeGreaterThan(0)
    })
  })

  it('breadcrumb links to the epic route', async () => {
    await renderTicketDetail(TASK_ID)
    const epicLink = await screen.findByRole('link', { name: /Ship v1 auth/i })
    // TanStack Router encodes the id in the href (e.g. ":" → "%3A")
    expect(epicLink).toHaveAttribute('href', expect.stringContaining(encodeURIComponent(EPIC_ID)))
  })
})

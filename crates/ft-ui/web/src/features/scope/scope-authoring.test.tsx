/**
 * Tests for the create/edit/delete/reorder authoring layer added to the scope
 * explorer (read-only behaviour is covered by scope-explorer.test.tsx).
 *
 * Each test asserts on the exact wire request the UI sends (method, URL, body)
 * via msw, then on the refresh that follows. The scaffold + empty-state tests
 * drive the progressive-disclosure paths off the (possibly empty) scope list.
 */
import { render, screen, waitFor, fireEvent, within } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from 'vitest'
import { setupServer } from 'msw/node'
import { http, HttpResponse } from 'msw'

vi.mock('@tanstack/react-router', () => ({
  Link: ({ children, ...rest }: React.PropsWithChildren<Record<string, unknown>>) => (
    <a {...(rest as Record<string, unknown>)}>{children}</a>
  ),
  useNavigate: () => () => {},
}))

import { ScopeExplorer } from './scope-explorer'

class FakeES {
  constructor(public url: string) {}
  close() {}
}
;(globalThis as { EventSource?: unknown }).EventSource = FakeES

type Scope = {
  id: string
  name: string
  appliesTo: string[]
  aliases: string[]
  hasCodeowners: boolean
}

const yaml = (s: Scope) => ({
  id: s.id,
  name: s.name || null,
  appliesTo: s.appliesTo,
  aliases: s.aliases,
  codeowners: null,
})

/** A mutable in-memory scope store so write ops re-render the list. */
function makeServer(initial: Scope[]) {
  let scopes = [...initial]
  const requests: { method: string; url: string; body: unknown }[] = []

  const server = setupServer(
    http.get('/api/scope', () => HttpResponse.json({ scopes })),
    http.get('/api/scope/aliases', () => HttpResponse.json({ aliases: [] })),
    http.get('/api/scope/preview', () =>
      HttpResponse.json({
        scopes: scopes.map((s, i) => ({ id: s.id, matchCount: i === 0 ? 0 : 12 })),
        warnings: scopes.length ? [`Scope "${scopes[0].id}" matches zero tracked files.`] : [],
      }),
    ),
    http.get('/api/files', ({ request }) => {
      const url = new URL(request.url)
      const dirs = url.searchParams.get('dirs')
      if (dirs === 'true' || dirs === '1') {
        return HttpResponse.json({ paths: ['apps/', 'packages/', 'crates/'] })
      }
      return HttpResponse.json({ paths: [] })
    }),
    http.post('/api/scope', async ({ request }) => {
      const body = (await request.json()) as Scope & { name: string | null }
      requests.push({ method: 'POST', url: '/api/scope', body })
      scopes = [
        ...scopes,
        {
          id: body.id,
          name: (body.name as string) ?? '',
          appliesTo: body.appliesTo,
          aliases: body.aliases ?? [],
          hasCodeowners: false,
        },
      ]
      return HttpResponse.json({ scopes: scopes.map(yaml) })
    }),
    http.put('/api/scope/:id', async ({ request, params }) => {
      const body = await request.json()
      requests.push({ method: 'PUT', url: `/api/scope/${params.id}`, body })
      return HttpResponse.json({ scopes: scopes.map(yaml) })
    }),
    http.delete('/api/scope/:id', ({ params }) => {
      requests.push({ method: 'DELETE', url: `/api/scope/${params.id}`, body: null })
      scopes = scopes.filter((s) => s.id !== params.id)
      return HttpResponse.json({ scopes: scopes.map(yaml) })
    }),
    http.post('/api/scope/reorder', async ({ request }) => {
      const body = (await request.json()) as { ids: string[] }
      requests.push({ method: 'POST', url: '/api/scope/reorder', body })
      const byId = new Map(scopes.map((s) => [s.id, s]))
      scopes = body.ids.map((id) => byId.get(id)!).filter(Boolean)
      return HttpResponse.json({ scopes: scopes.map(yaml) })
    }),
  )
  return { server, requests }
}

const TWO: Scope[] = [
  { id: 'core', name: 'Core', appliesTo: ['crates/ft-core/**'], aliases: ['ftc'], hasCodeowners: true },
  { id: 'ui', name: 'UI', appliesTo: ['crates/ft-ui/**'], aliases: [], hasCodeowners: false },
]

function renderExplorer() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(
    <QueryClientProvider client={qc}>
      <ScopeExplorer />
    </QueryClientProvider>,
  )
}

let active: ReturnType<typeof makeServer> | null = null
function start(initial: Scope[]) {
  active = makeServer(initial)
  active.server.listen({ onUnhandledRequest: 'bypass' })
  return active
}

beforeAll(() => {})
afterEach(() => {
  active?.server.close()
  active = null
})
afterAll(() => {})

describe('scope authoring', () => {
  it('creates a scope: posts ScopeInput then refreshes the list', async () => {
    const { requests } = start(TWO)
    renderExplorer()

    await waitFor(() => expect(screen.getByTestId('scope-list')).toHaveTextContent('core'))

    fireEvent.click(screen.getByTestId('scope-create-open'))
    fireEvent.change(screen.getByTestId('scope-form-id'), { target: { value: 'api' } })
    // First appliesTo row.
    fireEvent.change(screen.getByTestId('scope-form-applies-to'), {
      target: { value: 'crates/ft-api/**' },
    })
    fireEvent.click(screen.getByTestId('scope-form-submit'))

    await waitFor(() => {
      const post = requests.find((r) => r.method === 'POST' && r.url === '/api/scope')
      expect(post).toBeTruthy()
    })
    const post = requests.find((r) => r.method === 'POST' && r.url === '/api/scope')!
    expect(post.body).toMatchObject({
      id: 'api',
      appliesTo: ['crates/ft-api/**'],
      aliases: [],
    })
    // List refreshed with the new scope.
    await waitFor(() => expect(screen.getByTestId('scope-list')).toHaveTextContent('api'))
  })

  it('edits a scope: PUTs the changed fields', async () => {
    const { requests } = start(TWO)
    renderExplorer()
    await waitFor(() => expect(screen.getByTestId('scope-list')).toHaveTextContent('ui'))

    fireEvent.click(screen.getByTestId('scope-edit-ui'))
    const idField = screen.getByTestId('scope-form-applies-to') as HTMLInputElement
    fireEvent.change(idField, { target: { value: 'crates/ft-ui/web/**' } })
    fireEvent.click(screen.getByTestId('scope-form-submit'))

    await waitFor(() => {
      const put = requests.find((r) => r.method === 'PUT' && r.url === '/api/scope/ui')
      expect(put).toBeTruthy()
    })
    const put = requests.find((r) => r.method === 'PUT' && r.url === '/api/scope/ui')!
    expect(put.body).toMatchObject({ appliesTo: ['crates/ft-ui/web/**'] })
  })

  it('deletes a scope only after confirming in the alert dialog', async () => {
    const { requests } = start(TWO)
    renderExplorer()
    await waitFor(() => expect(screen.getByTestId('scope-list')).toHaveTextContent('core'))

    fireEvent.click(screen.getByTestId('scope-delete-core'))
    // Confirm in the dialog.
    const confirm = await screen.findByTestId('scope-delete-confirm')
    fireEvent.click(confirm)

    await waitFor(() => {
      const del = requests.find((r) => r.method === 'DELETE' && r.url === '/api/scope/core')
      expect(del).toBeTruthy()
    })
    await waitFor(() =>
      expect(screen.getByTestId('scope-list')).not.toHaveTextContent('core'),
    )
  })

  it('reorders: posts {ids} in the new order', async () => {
    const { requests } = start(TWO)
    renderExplorer()
    await waitFor(() => expect(screen.getByTestId('scope-list')).toHaveTextContent('ui'))

    // Move "core" (first) down → order becomes [ui, core].
    fireEvent.click(screen.getByTestId('scope-reorder-down-core'))

    await waitFor(() => {
      const reorder = requests.find((r) => r.url === '/api/scope/reorder')
      expect(reorder).toBeTruthy()
    })
    const reorder = requests.find((r) => r.url === '/api/scope/reorder')!
    expect(reorder.body).toEqual({ ids: ['ui', 'core'] })
  })

  it('renders preview match counts and a warning', async () => {
    start(TWO)
    renderExplorer()
    await waitFor(() => {
      expect(screen.getByTestId('scope-preview-match-ui')).toHaveTextContent('12')
    })
    expect(screen.getByTestId('scope-preview-match-core')).toHaveTextContent('0')
    expect(screen.getByTestId('scope-preview-warning')).toHaveTextContent(/zero tracked files/i)
  })

  it('shows a calm empty-state (no nag) when no scopes exist', async () => {
    start([])
    renderExplorer()
    await waitFor(() => {
      expect(screen.getByTestId('scope-empty-state')).toBeInTheDocument()
    })
    const empty = screen.getByTestId('scope-empty-state')
    expect(empty).toHaveTextContent(/single unit/i)
    // Opt-in CTA present, not an auto-opened form.
    expect(within(empty).getByTestId('scope-create-open')).toBeInTheDocument()
    expect(screen.queryByTestId('scope-form-submit')).not.toBeInTheDocument()
  })

  it('scaffold helper lists directory candidates from /api/files?dirs=1', async () => {
    start([])
    renderExplorer()
    await waitFor(() => expect(screen.getByTestId('scope-empty-state')).toBeInTheDocument())

    fireEvent.click(screen.getByTestId('scope-scaffold-open'))
    const candidates = await screen.findByTestId('scope-scaffold-candidates')
    // The trailing slash is stripped to form the candidate scope id.
    expect(candidates).toHaveTextContent('apps')
    expect(candidates).toHaveTextContent('packages')
    expect(candidates).toHaveTextContent('crates')
    // Each candidate has a confirm-to-create button (suggest-only).
    expect(within(candidates).getByTestId('scope-scaffold-confirm-apps')).toBeInTheDocument()
  })
})

vi.spyOn(console, 'error').mockImplementation(() => {})

import { render, screen, waitFor, fireEvent } from '@testing-library/react'
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

const scopes = [
  { id: 'core', name: 'Core', appliesTo: ['crates/ft-core/**'], aliases: ['ftc'], hasCodeowners: true },
  { id: 'ui', name: 'UI', appliesTo: ['crates/ft-ui/**'], aliases: [], hasCodeowners: false },
]

const server = setupServer(
  http.get('/api/scope', () => HttpResponse.json({ scopes })),
  http.get('/api/scope/aliases', () =>
    HttpResponse.json({ aliases: [{ alias: 'ftc', scopeId: 'core' }] }),
  ),
  http.get('/api/scope/core', () =>
    HttpResponse.json({
      scope: {
        summary: scopes[0],
        codeowners: [{ pattern: 'crates/ft-core/**', owners: ['@anuj'] }],
      },
    }),
  ),
  http.get('/api/scope/owners', ({ request }) => {
    const url = new URL(request.url)
    return HttpResponse.json({ path: url.searchParams.get('path') ?? '', owners: ['@anuj'] })
  }),
)
beforeAll(() => server.listen({ onUnhandledRequest: 'bypass' }))
afterEach(() => server.resetHandlers())
afterAll(() => server.close())

function renderExplorer(selectedId?: string) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(
    <QueryClientProvider client={qc}>
      <ScopeExplorer selectedId={selectedId} />
    </QueryClientProvider>,
  )
}

describe('<ScopeExplorer />', () => {
  it('renders the scope list and selected detail', async () => {
    renderExplorer('core')
    // List has "core" and "ui"; detail panel also shows "core" in breadcrumb,
    // so use the list testid to scope.
    await waitFor(() => {
      const list = screen.getByTestId('scope-list')
      expect(list).toHaveTextContent('core')
      expect(list).toHaveTextContent('ui')
    })
    await waitFor(() => {
      expect(screen.getByTestId('codeowners-table')).toBeInTheDocument()
    })
    const table = screen.getByTestId('codeowners-table')
    expect(table).toHaveTextContent('crates/ft-core/**')
    expect(table).toHaveTextContent('@anuj')
  })

  it('resolves a path → owners', async () => {
    renderExplorer()
    fireEvent.change(screen.getByPlaceholderText(/crates\/ft-core/i), {
      target: { value: 'crates/ft-core/src/lib.rs' },
    })
    fireEvent.click(screen.getByRole('button', { name: /resolve/i }))
    await waitFor(() => {
      expect(screen.getByTestId('owners-result')).toHaveTextContent('@anuj')
    })
  })
})

vi.spyOn(console, 'error').mockImplementation(() => {})

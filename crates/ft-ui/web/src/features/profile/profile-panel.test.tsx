import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { afterAll, afterEach, beforeAll, describe, expect, it } from 'vitest'
import { setupServer } from 'msw/node'
import { http, HttpResponse } from 'msw'
import { ProfilePanel } from './profile-panel'

class FakeES {
  constructor(public url: string) {}
  close() {}
}
;(globalThis as { EventSource?: unknown }).EventSource = FakeES

const baseProfile = {
  id: 'repo_profile:abc',
  validate_command: 'cargo test',
  test_command: null,
  build_command: null,
  lint_command: null,
  languages: ['rust'],
  package_managers: [],
  runtime: null,
  components: [{ name: 'ft-ui', path: 'crates/ft-ui', summary: null }],
  notes: null,
  trust: 'draft',
}

let putBody: unknown = null
/** Records the query string of every GET /api/profile the panel issues. */
const profileGets: string[] = []

const scopeDelta = {
  ...baseProfile,
  id: 'repo_profile:checkout',
  validate_command: null,
  test_command: 'pnpm test',
  trust: 'reviewed',
}
const scopeResolved = {
  ...scopeDelta,
  validate_command: 'cargo test', // inherited from base
}

const scopeList = {
  scopes: [
    {
      id: 'apps/checkout',
      name: 'apps/checkout',
      appliesTo: ['apps/checkout/**'],
      aliases: [],
      hasCodeowners: false,
    },
  ],
}

const server = setupServer(
  http.get('/api/scope', () => HttpResponse.json(scopeList)),
  http.get('/api/profile', ({ request }) => {
    const url = new URL(request.url)
    profileGets.push(url.search)
    const scope = url.searchParams.get('scope')
    const resolved = url.searchParams.get('resolved')
    if (scope === 'apps/checkout') {
      return HttpResponse.json(resolved ? scopeResolved : scopeDelta)
    }
    return HttpResponse.json(baseProfile)
  }),
  http.put('/api/profile', async ({ request }) => {
    putBody = await request.json()
    return HttpResponse.json({ ...baseProfile, validate_command: 'just ci' })
  }),
  http.delete('/api/profile/components/ft-ui', () =>
    HttpResponse.json({ ...baseProfile, components: [] }),
  ),
)
beforeAll(() => server.listen({ onUnhandledRequest: 'bypass' }))
afterEach(() => {
  server.resetHandlers()
  putBody = null
  profileGets.length = 0
})
afterAll(() => server.close())

function renderPanel() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(
    <QueryClientProvider client={qc}>
      <ProfilePanel />
    </QueryClientProvider>,
  )
}

describe('<ProfilePanel />', () => {
  it('renders commands, tooling, components, and trust', async () => {
    renderPanel()
    expect(await screen.findByTestId('profile-value-validateCommand')).toHaveTextContent(
      'cargo test',
    )
    expect(screen.getByTestId('profile-components')).toHaveTextContent('ft-ui')
    expect(screen.getByTestId('profile-components')).toHaveTextContent('crates/ft-ui')
  })

  it('persists an inline edit via PUT /api/profile', async () => {
    renderPanel()
    fireEvent.click(await screen.findByTestId('profile-edit-validateCommand'))
    const save = await screen.findByTestId('profile-save-validateCommand')
    fireEvent.click(save)
    await waitFor(() => expect(putBody).not.toBeNull())
    expect(putBody).toHaveProperty('validateCommand')
  })

  it('removes a component via the delete endpoint', async () => {
    renderPanel()
    const btn = await screen.findByTestId('profile-component-remove-ft-ui')
    fireEvent.click(btn)
    await waitFor(() =>
      expect(screen.getByText(/No components mapped yet/i)).toBeInTheDocument(),
    )
  })

  it('switches scope and refetches ?scope=, with a per-scope trust badge', async () => {
    renderPanel()
    // Wait for the base profile to load first.
    await screen.findByTestId('profile-value-validateCommand')

    // The switcher offers Base + the scope from GET /api/scope.
    const switcher = await screen.findByTestId('profile-scope-switcher')
    fireEvent.change(switcher, { target: { value: 'apps/checkout' } })

    // It refetches with ?scope=apps/checkout and shows the delta's command.
    await waitFor(() =>
      expect(profileGets.some((s) => s.includes('scope=apps%2Fcheckout'))).toBe(true),
    )
    await waitFor(() =>
      expect(screen.getByTestId('profile-value-testCommand')).toHaveTextContent('pnpm test'),
    )
    // The per-scope trust badge reflects the delta's trust (reviewed).
    expect(screen.getByTestId('profile-scope-trust')).toHaveTextContent(/reviewed/i)
  })

  it('toggling Resolved refetches ?resolved=1 and shows inherited fields', async () => {
    renderPanel()
    await screen.findByTestId('profile-value-validateCommand')

    fireEvent.change(await screen.findByTestId('profile-scope-switcher'), {
      target: { value: 'apps/checkout' },
    })
    await waitFor(() =>
      expect(screen.getByTestId('profile-value-testCommand')).toHaveTextContent('pnpm test'),
    )

    // Flip the Resolved toggle.
    fireEvent.click(screen.getByTestId('profile-resolved-toggle'))

    await waitFor(() => expect(profileGets.some((s) => s.includes('resolved=1'))).toBe(true))
    // Resolved view inherits the base validate command.
    await waitFor(() =>
      expect(screen.getByTestId('profile-value-validateCommand')).toHaveTextContent('cargo test'),
    )
  })
})

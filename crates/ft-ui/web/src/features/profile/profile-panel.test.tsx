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

const server = setupServer(
  http.get('/api/profile', () => HttpResponse.json(baseProfile)),
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
})

import { render, screen, waitFor, fireEvent } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from 'vitest'
import { setupServer } from 'msw/node'
import { http, HttpResponse } from 'msw'
import type { DocView } from '@/api/types/DocView'
import { DocsPanel } from './docs-panel'

// Tiptap/ProseMirror is awkward in jsdom — mock the shared editor down to a
// plain textarea/div so the panel's own logic (titles, badges, edit/save) is
// what's under test, not the editor internals.
vi.mock('@/components/markdown-editor', () => ({
  MarkdownEditor: ({
    value,
    onChange,
    editable,
  }: {
    value: string
    onChange?: (v: string) => void
    editable?: boolean
  }) =>
    // Mirror the real editor: editable unless explicitly `editable={false}`.
    editable !== false ? (
      <textarea
        data-testid="md-edit"
        value={value}
        onChange={(e) => onChange?.(e.target.value)}
      />
    ) : (
      <div data-testid="md-view">{value}</div>
    ),
  useMarkdownEditor: () => null,
}))

const TICKET = 'task:aaaa1111bbbb'

const fresh: DocView = {
  id: 'doc:fresh0001',
  title: 'Auth design',
  doc_type: 'design',
  path: 'docs/auth.md',
  summary: 'How auth works.',
  freshness: 'fresh',
  content: '# Auth design\n\nHow auth works.',
}
const stale: DocView = {
  id: 'doc:stale0002',
  title: 'Schema notes',
  doc_type: 'reference',
  path: 'docs/schema.md',
  summary: 'Tables.',
  freshness: 'stale',
  content: '# Schema notes\n\nTables.',
}
const missing: DocView = {
  id: 'doc:missing03',
  title: 'Gone doc',
  doc_type: 'adr',
  path: 'docs/gone.md',
  summary: '',
  freshness: 'missing',
  content: '',
}

const server = setupServer(
  http.get(`/api/tickets/${TICKET}/docs`, () => HttpResponse.json([fresh, stale, missing])),
)

beforeAll(() => server.listen({ onUnhandledRequest: 'bypass' }))
afterEach(() => server.resetHandlers())
afterAll(() => server.close())

function renderPanel() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(
    <QueryClientProvider client={qc}>
      <DocsPanel ticketId={TICKET} />
    </QueryClientProvider>,
  )
}

describe('<DocsPanel />', () => {
  it('lists linked docs and badges only the non-fresh ones', async () => {
    renderPanel()

    await waitFor(() => {
      expect(screen.getByText('Auth design')).toBeInTheDocument()
    })
    expect(screen.getByText('Schema notes')).toBeInTheDocument()
    expect(screen.getByText('Gone doc')).toBeInTheDocument()

    // The stale and missing docs carry a freshness badge; the fresh one doesn't.
    expect(screen.getByTestId('doc-badge-doc:stale0002')).toHaveTextContent(/stale/i)
    expect(screen.getByTestId('doc-badge-doc:missing03')).toHaveTextContent(/missing/i)
    expect(screen.queryByTestId('doc-badge-doc:fresh0001')).not.toBeInTheDocument()
  })

  it('shows an empty state when no docs are linked', async () => {
    server.use(http.get(`/api/tickets/${TICKET}/docs`, () => HttpResponse.json([])))
    renderPanel()
    expect(await screen.findByText(/no documentation linked/i)).toBeInTheDocument()
  })

  it('edits a doc through to the PUT endpoint and leaves edit mode', async () => {
    let putBody: { content: string } | null = null
    server.use(
      http.put(`/api/docs/${stale.id}/content`, async ({ request }) => {
        putBody = (await request.json()) as { content: string }
        return HttpResponse.json({ ...stale, freshness: 'fresh', content: putBody.content })
      }),
    )
    renderPanel()

    await waitFor(() => expect(screen.getByText('Schema notes')).toBeInTheDocument())

    // Enter edit mode on the stale doc.
    fireEvent.click(screen.getByTestId('doc-edit-doc:stale0002'))
    const textarea = await screen.findByTestId('md-edit')
    fireEvent.change(textarea, { target: { value: '# Schema notes\n\nEdited.' } })
    fireEvent.click(screen.getByTestId('doc-save-doc:stale0002'))

    await waitFor(() => expect(putBody).not.toBeNull())
    expect(putBody!.content).toContain('Edited.')
    // Editor closes after a successful save.
    await waitFor(() => expect(screen.queryByTestId('md-edit')).not.toBeInTheDocument())
  })
})

vi.spyOn(console, 'error').mockImplementation(() => {})

import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from 'vitest'
import { setupServer } from 'msw/node'
import { http, HttpResponse } from 'msw'

// Mock TanStack Router's Link so we don't need a RouterProvider in test scope.
vi.mock('@tanstack/react-router', () => ({
  Link: ({ children, ...rest }: React.PropsWithChildren<Record<string, unknown>>) => (
    <a {...(rest as Record<string, unknown>)}>{children}</a>
  ),
}))

import { LintView } from './lint-view'

class FakeES {
  constructor(public url: string) {}
  close() {}
}
;(globalThis as { EventSource?: unknown }).EventSource = FakeES

const server = setupServer(
  http.post('/api/audit/lint', () =>
    HttpResponse.json({
      scanned: 12,
      errors: 1,
      warnings: 1,
      findings: [
        {
          severity: 'error',
          rule: 'ac_cap_exceeded',
          recordId: 'task:abc',
          message: 'too many ACs',
          suggestedFix: 'split into subtasks',
        },
        {
          severity: 'warning',
          rule: 'missing_owner',
          recordId: 'memory:def',
          message: 'no owner',
          suggestedFix: null,
        },
      ],
    }),
  ),
)
beforeAll(() => server.listen({ onUnhandledRequest: 'bypass' }))
afterEach(() => server.resetHandlers())
afterAll(() => server.close())

function renderLint() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(
    <QueryClientProvider client={qc}>
      <LintView />
    </QueryClientProvider>,
  )
}

describe('<LintView />', () => {
  it('renders findings after running lint and filters by rule', async () => {
    renderLint()
    fireEvent.click(screen.getByTestId('lint-run'))
    await waitFor(() => {
      expect(screen.getByTestId('lint-findings')).toBeInTheDocument()
    })
    expect(screen.getByText('ac_cap_exceeded')).toBeInTheDocument()
    expect(screen.getByText('missing_owner')).toBeInTheDocument()

    fireEvent.change(screen.getByPlaceholderText('ac_cap_exceeded'), {
      target: { value: 'ac_cap' },
    })
    await waitFor(() => {
      expect(screen.queryByText('missing_owner')).toBeNull()
      expect(screen.getByText('ac_cap_exceeded')).toBeInTheDocument()
    })
  })
})

vi.spyOn(console, 'error').mockImplementation(() => {})

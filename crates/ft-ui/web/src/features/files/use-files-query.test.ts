import { renderHook, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { afterAll, afterEach, beforeAll, describe, expect, it } from 'vitest'
import { setupServer } from 'msw/node'
import { http, HttpResponse } from 'msw'
import { createElement, type ReactNode } from 'react'
import { useFiles } from './use-files-query'

/** Records the query string of every GET /api/files. */
const fileGets: string[] = []

const server = setupServer(
  http.get('/api/files', ({ request }) => {
    const url = new URL(request.url)
    fileGets.push(url.search)
    return HttpResponse.json({ paths: ['crates/ft-cli', 'crates/ft-ui'] })
  }),
)
beforeAll(() => server.listen({ onUnhandledRequest: 'bypass' }))
afterEach(() => {
  server.resetHandlers()
  fileGets.length = 0
})
afterAll(() => server.close())

function wrapper({ children }: { children: ReactNode }) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return createElement(QueryClientProvider, { client: qc }, children)
}

describe('useFiles', () => {
  it('GETs /api/files with prefix, dirs, and limit=50', async () => {
    const { result } = renderHook(() => useFiles('crates/', true), { wrapper })
    await waitFor(() => expect(result.current.data).toBeDefined())
    expect(result.current.data?.paths).toEqual(['crates/ft-cli', 'crates/ft-ui'])
    expect(fileGets.length).toBeGreaterThan(0)
    const q = fileGets[0]
    expect(q).toContain('prefix=crates%2F')
    expect(q).toContain('dirs=true')
    expect(q).toContain('limit=50')
  })

  it('passes dirs=false through', async () => {
    const { result } = renderHook(() => useFiles('src', false), { wrapper })
    await waitFor(() => expect(result.current.data).toBeDefined())
    expect(fileGets[0]).toContain('dirs=false')
  })
})

import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { afterAll, afterEach, beforeAll, describe, expect, it } from 'vitest'
import { setupServer } from 'msw/node'
import { http, HttpResponse } from 'msw'
import { useState } from 'react'
import { FilePathCombobox } from './autocomplete'

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

function Harness() {
  const [value, setValue] = useState('')
  return (
    <FilePathCombobox
      dirs
      value={value}
      onValueChange={setValue}
      data-testid="fpc"
    />
  )
}

function renderHarness() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(
    <QueryClientProvider client={qc}>
      <Harness />
    </QueryClientProvider>,
  )
}

describe('<FilePathCombobox />', () => {
  it('debounces the typed prefix into a GET /api/files?...&dirs=true and renders suggestions', async () => {
    renderHarness()
    const input = screen.getByTestId('fpc') as HTMLInputElement
    fireEvent.change(input, { target: { value: 'crates/' } })

    // Free-typed value is reflected immediately.
    expect(input.value).toBe('crates/')

    await waitFor(() =>
      expect(fileGets.some((s) => s.includes('prefix=crates%2F'))).toBe(true),
    )
    expect(fileGets.some((s) => s.includes('dirs=true'))).toBe(true)

    expect(await screen.findByText('crates/ft-ui')).toBeInTheDocument()
  })

  it('selecting a suggestion sets the value', async () => {
    renderHarness()
    const input = screen.getByTestId('fpc') as HTMLInputElement
    fireEvent.change(input, { target: { value: 'crates/' } })

    const option = await screen.findByText('crates/ft-cli')
    fireEvent.mouseDown(option)

    await waitFor(() => expect(input.value).toBe('crates/ft-cli'))
  })
})

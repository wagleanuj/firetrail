import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from 'vitest'
import { setupServer } from 'msw/node'
import { http, HttpResponse } from 'msw'
import { SalvageQueue } from './salvage-queue'

interface SalvageBody {
  dryRun?: boolean
  selected?: string[] | null
}

let calls: Array<{ dryRun: boolean; selected: string[] | null }> = []

const dryRunResponse = {
  base: 'main',
  sourceBranch: 'feature/x',
  sourceRef: 'feature/x',
  entries: [
    {
      id: 'memory:abc111',
      kind: 'memory',
      action: 'salvaged',
      reason: 'memory note from feature branch',
      path: 'memory/notes/abc111.md',
    },
    {
      id: 'memory:def222',
      kind: 'finding',
      action: 'skipped',
      reason: 'structural; promoted via ticket',
      path: 'memory/findings/def222.md',
    },
  ],
  salvageBranch: null,
  dryRun: true,
  warnings: [],
}

const applyResponse = {
  ...dryRunResponse,
  dryRun: false,
  salvageBranch: 'salvage/abc',
  entries: [dryRunResponse.entries[0]],
}

const server = setupServer(
  http.post('/api/memory/salvage', async ({ request }) => {
    const body = (await request.json()) as SalvageBody
    calls.push({ dryRun: !!body.dryRun, selected: body.selected ?? null })
    if (body.dryRun) return HttpResponse.json(dryRunResponse)
    return HttpResponse.json(applyResponse)
  }),
)

beforeAll(() => server.listen({ onUnhandledRequest: 'bypass' }))
afterEach(() => {
  server.resetHandlers()
  calls = []
})
afterAll(() => server.close())

class FakeES {
  url: string
  constructor(url: string) {
    this.url = url
  }
  close() {}
}
;(globalThis as { EventSource?: unknown }).EventSource = FakeES

function renderSalvage() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(
    <QueryClientProvider client={qc}>
      <SalvageQueue />
    </QueryClientProvider>,
  )
}

describe('<SalvageQueue />', () => {
  it('runs dry-run, allows selection, then applies', async () => {
    renderSalvage()

    fireEvent.click(screen.getByRole('button', { name: /run salvage scan/i }))
    await waitFor(() => {
      expect(screen.getByText('memory:abc111')).toBeInTheDocument()
    })
    expect(calls[0]?.dryRun).toBe(true)

    const checkbox = screen.getByLabelText(/select memory:abc111/i) as HTMLInputElement
    fireEvent.click(checkbox)
    expect(checkbox.checked).toBe(true)

    const acceptBtn = await screen.findByRole('button', { name: /accept selected \(1\)/i })
    fireEvent.click(acceptBtn)
    await waitFor(() => {
      expect(screen.getByRole('button', { name: /^apply$/i })).toBeInTheDocument()
    })
    fireEvent.click(screen.getByRole('button', { name: /^apply$/i }))

    await waitFor(() => {
      const applyCall = calls.find((c) => !c.dryRun)
      expect(applyCall).toBeDefined()
      expect(applyCall?.selected).toEqual(['memory:abc111'])
    })

    await waitFor(() => {
      const dryCalls = calls.filter((c) => c.dryRun)
      expect(dryCalls.length).toBeGreaterThanOrEqual(2)
    })
  })
})

vi.spyOn(console, 'error').mockImplementation(() => {})

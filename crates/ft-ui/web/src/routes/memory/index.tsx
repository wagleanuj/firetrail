import { useState } from 'react'
import { createFileRoute } from '@tanstack/react-router'
import type { MemoryKind } from '@/api/types/MemoryKind'
import type { TrustStateInput } from '@/api/types/TrustStateInput'
import { MemoryList } from '@/features/memory/memory-list'
import { CreateMemoryDialog } from '@/features/memory/create-dialog'

interface MemoryRouteSearch {
  kind?: MemoryKind
  trust?: TrustStateInput
  stale?: boolean
}

export const Route = createFileRoute('/memory/')({
  component: MemoryRoute,
  validateSearch: (search: Record<string, unknown>): MemoryRouteSearch => {
    const out: MemoryRouteSearch = {}
    if (typeof search.kind === 'string') out.kind = search.kind as MemoryKind
    if (typeof search.trust === 'string') out.trust = search.trust as TrustStateInput
    if (search.stale === true || search.stale === 'true') out.stale = true
    return out
  },
})

function MemoryRoute() {
  const [createOpen, setCreateOpen] = useState(false)
  return (
    <>
      <MemoryList onCreateClick={() => setCreateOpen(true)} />
      <CreateMemoryDialog open={createOpen} onOpenChange={setCreateOpen} />
    </>
  )
}

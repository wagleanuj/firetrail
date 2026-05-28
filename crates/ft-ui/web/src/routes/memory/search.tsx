import { createFileRoute } from '@tanstack/react-router'
import type { MemoryKind } from '@/api/types/MemoryKind'
import type { SearchMode } from '@/api/types/SearchMode'
import type { TrustStateInput } from '@/api/types/TrustStateInput'
import { MemorySearch, type SearchRouteParams } from '@/features/memory/memory-search'

export const Route = createFileRoute('/memory/search')({
  component: MemorySearch,
  validateSearch: (search: Record<string, unknown>): SearchRouteParams => {
    const out: SearchRouteParams = {}
    if (typeof search.q === 'string') out.q = search.q
    if (typeof search.mode === 'string') out.mode = search.mode as SearchMode
    if (typeof search.kind === 'string') out.kind = search.kind as MemoryKind
    if (typeof search.trust === 'string') out.trust = search.trust as TrustStateInput
    if (typeof search.scope === 'string') out.scope = search.scope
    if (search.includeQuarantine === true || search.includeQuarantine === 'true') {
      out.includeQuarantine = true
    }
    if (typeof search.similarTo === 'string') out.similarTo = search.similarTo
    return out
  },
})

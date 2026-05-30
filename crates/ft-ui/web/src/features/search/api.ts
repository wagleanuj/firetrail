/**
 * Thin wrapper around `apiFetch` for the unified cross-domain search route
 * (`GET /api/search`). Mirrors the memory feature's `api.ts` shape: a pure
 * function so it stays trivially mockable with MSW, with the caching layered
 * on top by the `useGlobalSearch` hook.
 */
import { apiFetch } from '@/api/client'
import type { GlobalSearchOutput } from '@/api/types/GlobalSearchOutput'
import type { SearchKind } from '@/api/types/SearchKind'
import type { SearchMode } from '@/api/types/SearchMode'
import type { TrustStateInput } from '@/api/types/TrustStateInput'

export interface GlobalSearchParams {
  q: string
  mode?: SearchMode
  /** Zero or more kinds to filter on. Empty = all kinds. */
  kinds?: SearchKind[]
  trust?: TrustStateInput | null
  scope?: string | null
  limit?: number
  includeQuarantine?: boolean
}

/** GET /api/search — unified cross-domain search. */
export function globalSearch(params: GlobalSearchParams): Promise<GlobalSearchOutput> {
  const q = new URLSearchParams()
  q.set('q', params.q)
  if (params.mode) q.set('mode', params.mode)
  if (params.kinds && params.kinds.length > 0) q.set('kind', params.kinds.join(','))
  if (params.trust) q.set('trust', params.trust)
  if (params.scope) q.set('scope', params.scope)
  if (params.limit) q.set('limit', String(params.limit))
  if (params.includeQuarantine) q.set('includeQuarantine', 'true')
  return apiFetch<GlobalSearchOutput>(`/api/search?${q.toString()}`)
}

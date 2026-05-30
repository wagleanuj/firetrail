/**
 * TanStack Query hook for the unified cross-domain search route. Used by the
 * Cmd+K command palette to surface live backend results alongside the static
 * nav shortcuts.
 */
import { useQuery, type UseQueryResult } from '@tanstack/react-query'
import type { GlobalSearchOutput } from '@/api/types/GlobalSearchOutput'
import { globalSearch, type GlobalSearchParams } from './api'

export const globalSearchKey = (p: GlobalSearchParams) =>
  [
    'global-search',
    p.q,
    p.mode ?? 'auto',
    (p.kinds ?? []).join(',') || null,
    p.trust ?? null,
    p.scope ?? null,
    p.limit ?? 20,
    p.includeQuarantine ?? false,
  ] as const

/**
 * Live cross-domain search. Disabled (no request) until the trimmed query is
 * non-empty, so an open-but-untouched palette never hits the network.
 */
export function useGlobalSearch(
  params: GlobalSearchParams,
  enabled = true,
): UseQueryResult<GlobalSearchOutput> {
  return useQuery({
    queryKey: globalSearchKey(params),
    queryFn: () => globalSearch(params),
    enabled: enabled && params.q.trim().length > 0,
    staleTime: 5_000,
  })
}

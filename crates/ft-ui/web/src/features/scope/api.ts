/**
 * Thin per-endpoint wrappers around `apiFetch` for the scope surface.
 *
 * The backend types ([`ScopeListOutput`], [`ScopeAliasesOutput`], …) use
 * verbose field names (`scopes`, `aliases`) — we hand-roll narrow wire
 * helpers here so the UI code can pretend it's a flat array.
 */
import { apiFetch } from '@/api/client'
import type { ScopeListOutput } from '@/api/types/ScopeListOutput'
import type { ScopeAliasesOutput } from '@/api/types/ScopeAliasesOutput'
import type { ScopeShowOutput } from '@/api/types/ScopeShowOutput'
import type { ScopeOwnersOutput } from '@/api/types/ScopeOwnersOutput'

/** GET /api/scope */
export function fetchScopes(): Promise<ScopeListOutput> {
  return apiFetch<ScopeListOutput>('/api/scope')
}

/** GET /api/scope/aliases */
export function fetchAliases(): Promise<ScopeAliasesOutput> {
  return apiFetch<ScopeAliasesOutput>('/api/scope/aliases')
}

/** GET /api/scope/:id */
export function fetchScope(id: string): Promise<ScopeShowOutput> {
  return apiFetch<ScopeShowOutput>(`/api/scope/${encodeURIComponent(id)}`)
}

/** GET /api/scope/owners?path= */
export function fetchOwners(path: string): Promise<ScopeOwnersOutput> {
  const q = new URLSearchParams({ path })
  return apiFetch<ScopeOwnersOutput>(`/api/scope/owners?${q.toString()}`)
}

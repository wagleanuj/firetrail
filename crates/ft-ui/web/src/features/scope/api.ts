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
import type { ScopeWriteOutput } from '@/api/types/ScopeWriteOutput'
import type { ScopePreviewView } from '@/api/types/ScopePreviewView'

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

/** GET /api/scope/preview — per-scope match counts + advisory warnings. */
export function fetchScopePreview(): Promise<ScopePreviewView> {
  return apiFetch<ScopePreviewView>('/api/scope/preview', { withRequestId: false })
}

/**
 * A new scope to declare (becomes last-declared → highest precedence). Mirrors
 * the backend `ScopeInput`; the UI passes `name`/`codeowners` as `null` when
 * blank and always sends an explicit (possibly empty) alias list.
 */
export interface AddScopeInput {
  id: string
  name?: string | null
  appliesTo: string[]
  aliases: string[]
  codeowners?: string | null
}

/** POST /api/scope — declare a new scope. */
export function addScope(input: AddScopeInput): Promise<ScopeWriteOutput> {
  return apiFetch<ScopeWriteOutput>('/api/scope', {
    method: 'POST',
    body: {
      id: input.id,
      name: input.name ?? null,
      appliesTo: input.appliesTo,
      aliases: input.aliases,
      codeowners: input.codeowners ?? null,
    },
  })
}

/**
 * A partial edit of an existing scope. Each field is optional: omit to leave
 * the stored value untouched. `name`/`codeowners` accept `null` to clear.
 */
export interface EditScopeInput {
  name?: string | null
  appliesTo?: string[]
  aliases?: string[]
  codeowners?: string | null
}

/** PUT /api/scope/:id — partial update of an existing scope. */
export function editScope(id: string, input: EditScopeInput): Promise<ScopeWriteOutput> {
  return apiFetch<ScopeWriteOutput>(`/api/scope/${encodeURIComponent(id)}`, {
    method: 'PUT',
    body: input,
  })
}

/** DELETE /api/scope/:id — remove a scope. */
export function removeScope(id: string): Promise<ScopeWriteOutput> {
  return apiFetch<ScopeWriteOutput>(`/api/scope/${encodeURIComponent(id)}`, {
    method: 'DELETE',
  })
}

/** POST /api/scope/reorder — set the full declaration order (last wins). */
export function reorderScopes(ids: string[]): Promise<ScopeWriteOutput> {
  return apiFetch<ScopeWriteOutput>('/api/scope/reorder', {
    method: 'POST',
    body: { ids },
  })
}

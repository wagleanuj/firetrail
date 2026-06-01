/**
 * TanStack Query hooks for the scope surface. All reads — scope is read-only
 * in V1 (CODEOWNERS edits happen via direct file edits).
 */
import {
  useQuery,
  useMutation,
  useQueryClient,
  type UseQueryResult,
} from '@tanstack/react-query'
import type { ScopeListOutput } from '@/api/types/ScopeListOutput'
import type { ScopeAliasesOutput } from '@/api/types/ScopeAliasesOutput'
import type { ScopeShowOutput } from '@/api/types/ScopeShowOutput'
import type { ScopeOwnersOutput } from '@/api/types/ScopeOwnersOutput'
import type { ScopePreviewView } from '@/api/types/ScopePreviewView'
import { toastApiError } from '@/api/error'
import {
  fetchScopes,
  fetchAliases,
  fetchScope,
  fetchOwners,
  fetchScopePreview,
  addScope,
  editScope,
  removeScope,
  reorderScopes,
  type AddScopeInput,
  type EditScopeInput,
} from './api'

export const scopeListKey = ['scope-list'] as const
export const scopeAliasesKey = ['scope-aliases'] as const
export const scopeShowKey = (id: string) => ['scope', id] as const
export const scopePreviewKey = ['scope-preview'] as const

export function useScopeList(): UseQueryResult<ScopeListOutput> {
  return useQuery({
    queryKey: scopeListKey,
    queryFn: fetchScopes,
    staleTime: 30_000,
  })
}

export function useScopeAliases(): UseQueryResult<ScopeAliasesOutput> {
  return useQuery({
    queryKey: scopeAliasesKey,
    queryFn: fetchAliases,
    staleTime: 30_000,
  })
}

export function useScopeShow(id: string | undefined): UseQueryResult<ScopeShowOutput> {
  return useQuery({
    queryKey: scopeShowKey(id ?? ''),
    queryFn: () => fetchScope(id!),
    enabled: !!id,
    staleTime: 10_000,
  })
}

/**
 * Path → owners resolver. Modelled as a `useMutation` because callers run it
 * imperatively (button click) rather than reactively.
 */
export function useResolveOwners() {
  return useMutation<ScopeOwnersOutput, unknown, string>({
    mutationFn: (path) => fetchOwners(path),
    onError: (err) => toastApiError(err, 'Path resolve failed'),
  })
}

/**
 * Live preview of the current scope set: per-scope match counts plus advisory
 * warnings (zero-match globs, broad-last shadowing). Refreshes whenever the
 * scope list is invalidated (scope-event listener) since it shares no key.
 */
export function useScopePreview(): UseQueryResult<ScopePreviewView> {
  return useQuery({
    queryKey: scopePreviewKey,
    queryFn: fetchScopePreview,
    staleTime: 10_000,
  })
}

/**
 * Invalidate every cache the scope surface depends on after a write. The write
 * endpoints return the full post-write list, but we re-query rather than write
 * the narrow `ScopeWriteOutput` into the rich list/preview shapes — simpler and
 * always consistent.
 */
function invalidateScope(qc: ReturnType<typeof useQueryClient>) {
  void qc.invalidateQueries({ queryKey: scopeListKey })
  void qc.invalidateQueries({ queryKey: scopeAliasesKey })
  void qc.invalidateQueries({ queryKey: scopePreviewKey })
  void qc.invalidateQueries({ queryKey: ['scope'] })
}

/** POST /api/scope — declare a new scope (becomes last-declared). */
export function useAddScope() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (input: AddScopeInput) => addScope(input),
    onSuccess: () => invalidateScope(qc),
    onError: (err) => toastApiError(err, 'Could not add scope'),
  })
}

/** PUT /api/scope/:id — partial edit of an existing scope. */
export function useEditScope() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: ({ id, input }: { id: string; input: EditScopeInput }) =>
      editScope(id, input),
    onSuccess: () => invalidateScope(qc),
    onError: (err) => toastApiError(err, 'Could not edit scope'),
  })
}

/** DELETE /api/scope/:id — remove a scope. */
export function useRemoveScope() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (id: string) => removeScope(id),
    onSuccess: () => invalidateScope(qc),
    onError: (err) => toastApiError(err, 'Could not delete scope'),
  })
}

/** POST /api/scope/reorder — set the full declaration order (last wins). */
export function useReorderScopes() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (ids: string[]) => reorderScopes(ids),
    onSuccess: () => invalidateScope(qc),
    onError: (err) => toastApiError(err, 'Could not reorder scopes'),
  })
}

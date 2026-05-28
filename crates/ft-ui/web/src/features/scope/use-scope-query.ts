/**
 * TanStack Query hooks for the scope surface. All reads — scope is read-only
 * in V1 (CODEOWNERS edits happen via direct file edits).
 */
import { useQuery, useMutation, type UseQueryResult } from '@tanstack/react-query'
import type { ScopeListOutput } from '@/api/types/ScopeListOutput'
import type { ScopeAliasesOutput } from '@/api/types/ScopeAliasesOutput'
import type { ScopeShowOutput } from '@/api/types/ScopeShowOutput'
import type { ScopeOwnersOutput } from '@/api/types/ScopeOwnersOutput'
import { toastApiError } from '@/api/error'
import { fetchScopes, fetchAliases, fetchScope, fetchOwners } from './api'

export const scopeListKey = ['scope-list'] as const
export const scopeAliasesKey = ['scope-aliases'] as const
export const scopeShowKey = (id: string) => ['scope', id] as const

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

/**
 * TanStack Query hooks for the repo-profile surface.
 *
 * The singleton profile lives under a single query key; every mutation writes
 * the returned `ProfileView` straight into the cache so the panel reflects the
 * server's truth without a refetch. SSE `profile_updated` events from other
 * clients invalidate the key (wired in the panel).
 */
import {
  useMutation,
  useQuery,
  useQueryClient,
  type UseMutationResult,
  type UseQueryResult,
} from '@tanstack/react-query'
import type { ProfileView } from '@/api/types/ProfileView'
import type { ScopeSummary } from '@/api/types/ScopeSummary'
import type { ValidatePlanView } from '@/api/types/ValidatePlanView'
import { toastApiError } from '@/api/error'
import {
  addComponent,
  fetchProfile,
  fetchScopes,
  removeComponent,
  resolveValidatePlan,
  updateProfile,
  type ProfilePatch,
  type ProfileSelector,
  type ResolveInput,
} from './api'

/** The base profile lives under the root key; scopes nest a selector under it. */
export const profileKey = ['profile'] as const

/** Query key for a specific selector (base when `scope` is null/absent). */
export function profileSelectorKey(selector: ProfileSelector) {
  return [...profileKey, selector.scope ?? null, Boolean(selector.resolved)] as const
}

export const scopesKey = ['profile', 'scopes'] as const

export function useProfileQuery(
  selector: ProfileSelector = {},
): UseQueryResult<ProfileView | null> {
  return useQuery({
    queryKey: profileSelectorKey(selector),
    queryFn: () => fetchProfile(selector),
    staleTime: 15_000,
  })
}

/** The scope list that feeds the panel's scope switcher. */
export function useScopesQuery(): UseQueryResult<ScopeSummary[]> {
  return useQuery({
    queryKey: scopesKey,
    queryFn: fetchScopes,
    staleTime: 60_000,
  })
}

export function useUpdateProfile(selector: ProfileSelector = {}) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (patch: ProfilePatch) => updateProfile(patch, selector),
    onSuccess: (view) => qc.setQueryData(profileSelectorKey(selector), view),
    onError: (err) => toastApiError(err),
  })
}

/**
 * Resolve a validate plan on demand. Modelled as a mutation so it only fires
 * when the user submits (clicks Resolve / Use staged diff) rather than on every
 * keystroke; `data` holds the latest `ValidatePlanView`.
 */
export function useResolve(): UseMutationResult<ValidatePlanView, Error, ResolveInput> {
  return useMutation({
    mutationFn: (input: ResolveInput) => resolveValidatePlan(input),
    onError: (err) => toastApiError(err),
  })
}

export function useAddComponent() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (input: { name: string; path: string; summary?: string }) =>
      addComponent(input.name, input.path, input.summary),
    onSuccess: (view) => qc.setQueryData(profileSelectorKey({}), view),
    onError: (err) => toastApiError(err),
  })
}

export function useRemoveComponent() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (name: string) => removeComponent(name),
    onSuccess: (view) => qc.setQueryData(profileSelectorKey({}), view),
    onError: (err) => toastApiError(err),
  })
}

/**
 * Query + mutation hooks for the identity surface.
 */
import { useMutation, useQuery, useQueryClient, type UseQueryResult } from '@tanstack/react-query'
import { toast } from 'sonner'
import { toastApiError } from '@/api/error'
import type { IdentityListOutput } from '@/api/types/IdentityListOutput'
import type { IdentityShowOutput } from '@/api/types/IdentityShowOutput'
import type { IdentityCapabilitiesOutput } from '@/api/types/IdentityCapabilitiesOutput'
import {
  fetchIdentities,
  fetchIdentity,
  fetchCapabilities,
  registerIdentity,
  offboardIdentity,
  updateCapabilities,
  type CapabilityPatch,
  type IdentityListFilters,
  type IdentityRegisterBody,
  type IdentityOffboardResult,
} from './api'

export const identityListKey = (filters: IdentityListFilters = {}) =>
  [
    'identity-list',
    filters.kind ?? null,
    filters.status ?? null,
    filters.limit ?? null,
    filters.offset ?? null,
  ] as const

export const identityShowKey = (id: string) => ['identity', id] as const
export const identityCapsKey = (id: string) => ['identity-caps', id] as const

export function useIdentityList(
  filters: IdentityListFilters = {},
): UseQueryResult<IdentityListOutput> {
  return useQuery({
    queryKey: identityListKey(filters),
    queryFn: () => fetchIdentities(filters),
    staleTime: 10_000,
  })
}

export function useIdentity(id: string | undefined): UseQueryResult<IdentityShowOutput> {
  return useQuery({
    queryKey: identityShowKey(id ?? ''),
    queryFn: () => fetchIdentity(id!),
    enabled: !!id,
    staleTime: 10_000,
  })
}

export function useCapabilities(
  id: string | undefined,
): UseQueryResult<IdentityCapabilitiesOutput> {
  return useQuery({
    queryKey: identityCapsKey(id ?? ''),
    queryFn: () => fetchCapabilities(id!),
    enabled: !!id,
    staleTime: 10_000,
  })
}

export function useRegisterIdentity() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (body: IdentityRegisterBody) => registerIdentity(body),
    onSuccess: (out) => {
      toast.success(`Registered ${out.identity.id}`)
      qc.invalidateQueries({ queryKey: ['identity-list'] })
    },
    onError: (err) => toastApiError(err, 'Register failed'),
  })
}

export function useUpdateCapabilities(id: string) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (patches: CapabilityPatch[]) => updateCapabilities(id, patches),
    onSuccess: () => {
      toast.success('Capabilities updated')
      qc.invalidateQueries({ queryKey: identityCapsKey(id) })
      qc.invalidateQueries({ queryKey: identityShowKey(id) })
    },
    onError: (err) => toastApiError(err, 'Update failed'),
  })
}

export function useOffboardIdentity(id: string) {
  const qc = useQueryClient()
  return useMutation<IdentityOffboardResult, unknown, void>({
    mutationFn: () => offboardIdentity(id),
    onSuccess: (out) => {
      toast.success(
        `Offboarded ${out.identity.id} (${out.claimsReleased} claim${out.claimsReleased === 1 ? '' : 's'} released)`,
      )
      qc.invalidateQueries({ queryKey: ['identity-list'] })
      qc.invalidateQueries({ queryKey: identityShowKey(id) })
      qc.invalidateQueries({ queryKey: identityCapsKey(id) })
    },
    onError: (err) => toastApiError(err, 'Offboard failed'),
  })
}

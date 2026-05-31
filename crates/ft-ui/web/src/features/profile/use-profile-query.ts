/**
 * TanStack Query hooks for the repo-profile surface.
 *
 * The singleton profile lives under a single query key; every mutation writes
 * the returned `ProfileView` straight into the cache so the panel reflects the
 * server's truth without a refetch. SSE `profile_updated` events from other
 * clients invalidate the key (wired in the panel).
 */
import { useMutation, useQuery, useQueryClient, type UseQueryResult } from '@tanstack/react-query'
import type { ProfileView } from '@/api/types/ProfileView'
import { toastApiError } from '@/api/error'
import {
  addComponent,
  fetchProfile,
  removeComponent,
  updateProfile,
  type ProfilePatch,
} from './api'

export const profileKey = ['profile'] as const

export function useProfileQuery(): UseQueryResult<ProfileView | null> {
  return useQuery({
    queryKey: profileKey,
    queryFn: fetchProfile,
    staleTime: 15_000,
  })
}

export function useUpdateProfile() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (patch: ProfilePatch) => updateProfile(patch),
    onSuccess: (view) => qc.setQueryData(profileKey, view),
    onError: (err) => toastApiError(err),
  })
}

export function useAddComponent() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (input: { name: string; path: string; summary?: string }) =>
      addComponent(input.name, input.path, input.summary),
    onSuccess: (view) => qc.setQueryData(profileKey, view),
    onError: (err) => toastApiError(err),
  })
}

export function useRemoveComponent() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (name: string) => removeComponent(name),
    onSuccess: (view) => qc.setQueryData(profileKey, view),
    onError: (err) => toastApiError(err),
  })
}

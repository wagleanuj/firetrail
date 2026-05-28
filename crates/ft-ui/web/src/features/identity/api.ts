/**
 * Identity surface — list / show / register / offboard / capabilities.
 *
 * The backend's list endpoint accepts `kind`, `status`, `limit`, `offset` as
 * query params; we surface a typed `IdentityListFilters` object.
 */
import { apiFetch } from '@/api/client'
import type { IdentityKindInput } from '@/api/types/IdentityKindInput'
import type { IdentityStatusFilter } from '@/api/types/IdentityStatusFilter'
import type { IdentityListOutput } from '@/api/types/IdentityListOutput'
import type { IdentityShowOutput } from '@/api/types/IdentityShowOutput'
import type { IdentityRegisterInput } from '@/api/types/IdentityRegisterInput'
import type { IdentityRegisterOutput } from '@/api/types/IdentityRegisterOutput'
import type { IdentityCapabilitiesOutput } from '@/api/types/IdentityCapabilitiesOutput'

export interface IdentityListFilters {
  kind?: IdentityKindInput | null
  status?: IdentityStatusFilter | null
  limit?: number | null
  offset?: number | null
}

export function fetchIdentities(
  filters: IdentityListFilters = {},
): Promise<IdentityListOutput> {
  const q = new URLSearchParams()
  if (filters.kind) q.set('kind', filters.kind)
  if (filters.status) q.set('status', filters.status)
  if (filters.limit) q.set('limit', String(filters.limit))
  if (filters.offset) q.set('offset', String(filters.offset))
  const qs = q.toString()
  return apiFetch<IdentityListOutput>(`/api/identity${qs ? `?${qs}` : ''}`)
}

export function fetchIdentity(id: string): Promise<IdentityShowOutput> {
  return apiFetch<IdentityShowOutput>(`/api/identity/${encodeURIComponent(id)}`)
}

export type IdentityRegisterBody = Omit<IdentityRegisterInput, 'requestId'>

export function registerIdentity(
  body: IdentityRegisterBody,
): Promise<IdentityRegisterOutput> {
  return apiFetch<IdentityRegisterOutput>('/api/identity', {
    method: 'POST',
    body,
  })
}

export interface IdentityOffboardResult {
  identity: IdentityShowOutput['identity']
  claimsReleased: number
}

export function offboardIdentity(id: string): Promise<IdentityOffboardResult> {
  return apiFetch<IdentityOffboardResult>(
    `/api/identity/${encodeURIComponent(id)}/offboard`,
    { method: 'POST', body: { sweepClaims: true } },
  )
}

export function fetchCapabilities(
  id: string,
): Promise<IdentityCapabilitiesOutput> {
  return apiFetch<IdentityCapabilitiesOutput>(
    `/api/identity/${encodeURIComponent(id)}/capabilities`,
  )
}

export interface CapabilityPatch {
  /** Capability key (e.g. `can_promote_verified`). */
  key: string
  /** New value, or `null` to clear the override. */
  value: boolean | null
}

export interface UpdateCapabilitiesResult {
  identity: IdentityShowOutput['identity']
}

export function updateCapabilities(
  id: string,
  patches: CapabilityPatch[],
): Promise<UpdateCapabilitiesResult> {
  return apiFetch<UpdateCapabilitiesResult>(
    `/api/identity/${encodeURIComponent(id)}/capabilities`,
    {
      method: 'PATCH',
      body: { capabilities: patches },
    },
  )
}

/**
 * Thin per-endpoint wrappers around `apiFetch` for the memory surface.
 *
 * Mirror the tickets feature shape: pure functions (no React hooks) so they
 * stay trivially mockable with MSW. The `useXxx` hooks in this directory
 * layer caching + optimism on top via TanStack Query.
 */
import { apiFetch } from '@/api/client'
import type { MemoryKind } from '@/api/types/MemoryKind'
import type { MemoryListOutput } from '@/api/types/MemoryListOutput'
import type { MemoryRowOut } from '@/api/types/MemoryRowOut'
import type { SalvageInput } from '@/api/types/SalvageInput'
import type { SalvageOutput } from '@/api/types/SalvageOutput'
import type { SearchMode } from '@/api/types/SearchMode'
import type { SearchOutput } from '@/api/types/SearchOutput'
import type { TrustStateInput } from '@/api/types/TrustStateInput'
import type { RecordWire } from '@/api/wire/record'
import type { MemoryCreateBody } from './types'

export interface MemoryListFilters {
  kind?: MemoryKind | null
  trust?: TrustStateInput | null
  stale?: boolean
  limit?: number | null
}

export interface MemoryListResponse {
  rows: MemoryRowOut[]
}

/** GET /api/memory */
export function fetchMemoryList(filters: MemoryListFilters = {}): Promise<MemoryListResponse> {
  const q = new URLSearchParams()
  if (filters.kind) q.set('kind', filters.kind)
  if (filters.trust) q.set('trust', filters.trust)
  if (filters.stale) q.set('stale', 'true')
  if (filters.limit) q.set('limit', String(filters.limit))
  const qs = q.toString()
  return apiFetch<MemoryListOutput>(`/api/memory${qs ? `?${qs}` : ''}`)
}

/** GET /api/memory/stale */
export function fetchMemoryStale(kind?: MemoryKind | null): Promise<MemoryListResponse> {
  const q = new URLSearchParams()
  if (kind) q.set('kind', kind)
  const qs = q.toString()
  return apiFetch<MemoryListOutput>(`/api/memory/stale${qs ? `?${qs}` : ''}`)
}

/** Wire-level shape of `ft_ops::memory::views::ShowOutput`. */
export interface MemoryShowResponse {
  record: RecordWire
}

/** GET /api/memory/:id */
export function fetchMemory(id: string): Promise<MemoryShowResponse> {
  return apiFetch<MemoryShowResponse>(`/api/memory/${encodeURIComponent(id)}`)
}

export interface CreateMemoryResponse {
  record: RecordWire
}

/** POST /api/memory */
export function createMemory(body: MemoryCreateBody): Promise<CreateMemoryResponse> {
  return apiFetch<CreateMemoryResponse>('/api/memory', { method: 'POST', body })
}

export interface SearchParams {
  q: string
  mode?: SearchMode
  kind?: MemoryKind | null
  trust?: TrustStateInput | null
  scope?: string | null
  limit?: number
  includeQuarantine?: boolean
}

/** GET /api/memory/search */
export function searchMemory(params: SearchParams): Promise<SearchOutput> {
  const q = new URLSearchParams()
  q.set('q', params.q)
  if (params.mode) q.set('mode', params.mode)
  if (params.kind) q.set('kind', params.kind)
  if (params.trust) q.set('trust', params.trust)
  if (params.scope) q.set('scope', params.scope)
  if (params.limit) q.set('limit', String(params.limit))
  if (params.includeQuarantine) q.set('includeQuarantine', 'true')
  return apiFetch<SearchOutput>(`/api/memory/search?${q.toString()}`)
}

/** GET /api/memory/similar/:id */
export function similarMemory(id: string, limit = 10): Promise<SearchOutput> {
  const q = new URLSearchParams()
  q.set('limit', String(limit))
  return apiFetch<SearchOutput>(`/api/memory/similar/${encodeURIComponent(id)}?${q.toString()}`)
}

/** POST /api/memory/salvage */
export function postSalvage(
  body: Partial<Omit<SalvageInput, 'requestId'>> = {},
): Promise<SalvageOutput> {
  const payload: Partial<SalvageInput> = {
    base: body.base ?? 'main',
    branch: body.branch ?? null,
    dryRun: body.dryRun ?? false,
    selected: body.selected ?? null,
    requestId: null,
  }
  return apiFetch<SalvageOutput>('/api/memory/salvage', { method: 'POST', body: payload })
}

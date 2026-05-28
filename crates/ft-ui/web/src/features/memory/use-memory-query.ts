/**
 * TanStack Query hooks for the memory surface (reads).
 */
import { useQuery, type UseQueryResult } from '@tanstack/react-query'
import type { MemoryKind } from '@/api/types/MemoryKind'
import type { TrustStateInput } from '@/api/types/TrustStateInput'
import type { SearchOutput } from '@/api/types/SearchOutput'
import {
  fetchMemory,
  fetchMemoryList,
  fetchMemoryStale,
  searchMemory,
  similarMemory,
  type MemoryListFilters,
  type MemoryListResponse,
  type MemoryShowResponse,
  type SearchParams,
} from './api'

export const memoryListKey = (filters: MemoryListFilters = {}) =>
  [
    'memory-list',
    filters.kind ?? null,
    filters.trust ?? null,
    filters.stale ?? false,
    filters.limit ?? null,
  ] as const

export const memoryShowKey = (id: string) => ['memory', id] as const
export const memorySearchKey = (p: SearchParams) =>
  [
    'memory-search',
    p.q,
    p.mode ?? 'auto',
    p.kind ?? null,
    p.trust ?? null,
    p.scope ?? null,
    p.limit ?? 20,
    p.includeQuarantine ?? false,
  ] as const
export const memorySimilarKey = (id: string, limit: number) =>
  ['memory-similar', id, limit] as const

/** Either `/api/memory` or `/api/memory/stale` depending on `stale`. */
export function useMemoryList(
  filters: MemoryListFilters = {},
): UseQueryResult<MemoryListResponse> {
  return useQuery({
    queryKey: memoryListKey(filters),
    queryFn: () =>
      filters.stale ? fetchMemoryStale(filters.kind ?? null) : fetchMemoryList(filters),
    staleTime: 5_000,
  })
}

export function useMemoryQuery(
  id: string | undefined,
): UseQueryResult<MemoryShowResponse> {
  return useQuery({
    queryKey: memoryShowKey(id ?? ''),
    queryFn: () => fetchMemory(id!),
    enabled: !!id,
    staleTime: 10_000,
  })
}

export function useMemorySearch(
  params: SearchParams,
  enabled: boolean,
): UseQueryResult<SearchOutput> {
  return useQuery({
    queryKey: memorySearchKey(params),
    queryFn: () => searchMemory(params),
    enabled: enabled && params.q.trim().length > 0,
    staleTime: 5_000,
  })
}

export function useMemorySimilar(
  id: string | undefined,
  limit = 10,
): UseQueryResult<SearchOutput> {
  return useQuery({
    queryKey: memorySimilarKey(id ?? '', limit),
    queryFn: () => similarMemory(id!, limit),
    enabled: !!id,
    staleTime: 5_000,
  })
}

export type { MemoryKind, TrustStateInput }

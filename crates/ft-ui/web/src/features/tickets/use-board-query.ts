import { useQuery, type UseQueryResult } from '@tanstack/react-query'
import type { BoardOutput } from '@/api/types/BoardOutput'
import { fetchBoard, type BoardFilters } from './api'

export const boardQueryKey = (filters: BoardFilters = {}) =>
  ['board', filters.scope ?? null, filters.owner ?? null] as const

export function useBoardQuery(filters: BoardFilters = {}): UseQueryResult<BoardOutput> {
  return useQuery({
    queryKey: boardQueryKey(filters),
    queryFn: () => fetchBoard(filters),
    staleTime: 5_000,
  })
}

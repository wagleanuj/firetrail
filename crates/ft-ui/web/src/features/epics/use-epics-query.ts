import { useQuery, type UseQueryResult } from '@tanstack/react-query'
import type { EpicsOutput } from '@/api/types/EpicsOutput'
import { fetchEpics } from './api'

export const epicsQueryKey = ['epics'] as const

export function useEpicsQuery(): UseQueryResult<EpicsOutput> {
  return useQuery({
    queryKey: epicsQueryKey,
    queryFn: fetchEpics,
    staleTime: 5_000,
  })
}

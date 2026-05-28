import { useQuery, type UseQueryResult } from '@tanstack/react-query'
import type { ShowOutputWire } from '@/api/wire/record'
import { fetchTicket } from './api'

export const ticketQueryKey = (id: string) => ['ticket', id] as const

export function useTicketQuery(
  id: string | undefined,
): UseQueryResult<ShowOutputWire> {
  return useQuery({
    queryKey: ticketQueryKey(id ?? ''),
    queryFn: () => fetchTicket(id!),
    enabled: !!id,
    staleTime: 5_000,
  })
}

/**
 * TanStack Query hooks for the ticket-drawer Docs panel.
 *
 * The list query is keyed by ticket id; a successful edit invalidates it so the
 * freshness badge re-derives from the server (a save flips stale → fresh).
 */
import {
  useMutation,
  useQuery,
  useQueryClient,
  type UseMutationResult,
  type UseQueryResult,
} from '@tanstack/react-query'
import { toast } from 'sonner'
import { toastApiError } from '@/api/error'
import type { DocView } from '@/api/types/DocView'
import { fetchTicketDocs, saveDoc } from './docs-api'

export const ticketDocsQueryKey = (ticketId: string) => ['ticket-docs', ticketId] as const

export function useTicketDocsQuery(ticketId: string | undefined): UseQueryResult<DocView[]> {
  return useQuery({
    queryKey: ticketDocsQueryKey(ticketId ?? ''),
    queryFn: () => fetchTicketDocs(ticketId!),
    enabled: !!ticketId,
    staleTime: 5_000,
  })
}

interface SaveDocVars {
  docId: string
  content: string
}

export function useSaveDoc(
  ticketId: string,
): UseMutationResult<DocView, unknown, SaveDocVars> {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: ({ docId, content }: SaveDocVars) => saveDoc(docId, content),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ticketDocsQueryKey(ticketId) })
      toast.success('Doc saved')
    },
    onError: (err) => toastApiError(err),
  })
}

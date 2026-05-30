/**
 * Doc-panel endpoints (firetrail-2mwp.8). Pure `apiFetch` wrappers — the
 * caching/optimism layer lives in `use-ticket-docs.ts`.
 *
 *   GET  /api/tickets/:id/docs   → the ticket's DocumentedIn docs + freshness
 *   PUT  /api/docs/:id/content   → write new content through + re-index
 */
import { apiFetch } from '@/api/client'
import type { DocView } from '@/api/types/DocView'

export function fetchTicketDocs(ticketId: string): Promise<DocView[]> {
  return apiFetch<DocView[]>(`/api/tickets/${encodeURIComponent(ticketId)}/docs`)
}

export function saveDoc(docId: string, content: string): Promise<DocView> {
  return apiFetch<DocView>(`/api/docs/${encodeURIComponent(docId)}/content`, {
    method: 'PUT',
    body: { content },
  })
}

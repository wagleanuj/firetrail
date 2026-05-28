/**
 * Thin per-endpoint wrappers around `apiFetch` for the tickets surface.
 *
 * Keep these pure (no React hooks) so they're trivially testable and reusable
 * from Vitest. The `useXxx` hooks in this directory layer caching + optimism
 * on top via TanStack Query.
 */
import { apiFetch } from '@/api/client'
import type { BoardOutput } from '@/api/types/BoardOutput'
import type { ClaimInput } from '@/api/types/ClaimInput'
import type { CreateBugInput } from '@/api/types/CreateBugInput'
import type { CreateEpicInput } from '@/api/types/CreateEpicInput'
import type { CreateSubtaskInput } from '@/api/types/CreateSubtaskInput'
import type { CreateTaskInput } from '@/api/types/CreateTaskInput'
import type { TicketRelationKind } from '@/api/types/TicketRelationKind'
import type {
  ClaimOutputWire,
  CloseOutputWire,
  CreatedTicketWire,
  ShowOutputWire,
  UnclaimOutputWire,
  UpdateOutputWire,
} from '@/api/wire/record'

export interface BoardFilters {
  scope?: string | null
  owner?: string | null
}

export function fetchBoard(filters: BoardFilters = {}): Promise<BoardOutput> {
  const q = new URLSearchParams()
  if (filters.scope) q.set('scope', filters.scope)
  if (filters.owner) q.set('owner', filters.owner)
  const qs = q.toString()
  return apiFetch<BoardOutput>(`/api/tickets/board${qs ? `?${qs}` : ''}`)
}

export function fetchTicket(id: string): Promise<ShowOutputWire> {
  return apiFetch<ShowOutputWire>(`/api/tickets/${encodeURIComponent(id)}`)
}

export type CreateBody =
  | ({ kind: 'epic' } & Omit<CreateEpicInput, 'request_id'>)
  | ({ kind: 'task' } & Omit<CreateTaskInput, 'request_id'>)
  | ({ kind: 'subtask' } & Omit<CreateSubtaskInput, 'request_id'>)
  | ({ kind: 'bug' } & Omit<CreateBugInput, 'request_id'>)

export function createTicket(body: CreateBody): Promise<CreatedTicketWire> {
  return apiFetch<CreatedTicketWire>('/api/tickets', { method: 'POST', body })
}

export interface UpdatePatch {
  title?: string
  status?:
    | 'open'
    | 'ready'
    | 'in_progress'
    | 'review'
    | 'blocked'
    | 'closed'
    | 'deferred'
    | 'archived'
  priority?: 'p0' | 'p1' | 'p2' | 'p3' | 'p4'
  owner?: string
  description?: string
}

export function updateTicket(id: string, patch: UpdatePatch): Promise<UpdateOutputWire> {
  return apiFetch<UpdateOutputWire>(`/api/tickets/${encodeURIComponent(id)}`, {
    method: 'PATCH',
    body: patch,
  })
}

export function claimTicket(id: string, body?: Pick<ClaimInput, 'expires'>): Promise<ClaimOutputWire> {
  return apiFetch<ClaimOutputWire>(`/api/tickets/${encodeURIComponent(id)}/claim`, {
    method: 'POST',
    body: body ?? {},
  })
}

export function unclaimTicket(
  id: string,
  body?: { takeover?: boolean; reason?: string | null },
): Promise<UnclaimOutputWire> {
  return apiFetch<UnclaimOutputWire>(`/api/tickets/${encodeURIComponent(id)}/unclaim`, {
    method: 'POST',
    body: body ?? {},
  })
}

export function closeTicket(
  id: string,
  body?: { force?: boolean; reason?: string | null },
): Promise<CloseOutputWire> {
  return apiFetch<CloseOutputWire>(`/api/tickets/${encodeURIComponent(id)}/close`, {
    method: 'POST',
    body: body ?? {},
  })
}

export interface LinkBody {
  to: string
  kind: TicketRelationKind
}

export function linkTicket(id: string, body: LinkBody): Promise<unknown> {
  return apiFetch(`/api/tickets/${encodeURIComponent(id)}/links`, {
    method: 'POST',
    body,
  })
}

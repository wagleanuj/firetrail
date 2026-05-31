/**
 * Ticket mutation hooks. Each one:
 *   - performs the network round-trip via `./api`
 *   - applies an optimistic cache patch where it makes sense
 *   - falls back to invalidate-on-success (cheaper than re-writing the cache
 *     for low-frequency ops like create/link)
 *   - surfaces typed errors via `toastApiError`
 */
import { useMutation, useQueryClient, type UseMutationResult } from '@tanstack/react-query'
import { toast } from 'sonner'
import type { BoardOutput } from '@/api/types/BoardOutput'
import type { BoardCard } from '@/api/types/BoardCard'
import { toastApiError } from '@/api/error'
import {
  claimTicket,
  closeTicket,
  createTicket,
  linkTicket,
  reopenTicket,
  unclaimTicket,
  updateTicket,
  type CreateBody,
  type LinkBody,
  type UpdatePatch,
} from './api'
import type { ClaimOutputWire, CloseOutputWire, CreatedTicketWire, UnclaimOutputWire, UpdateOutputWire } from '@/api/wire/record'
import { ticketQueryKey } from './use-ticket-query'

type Column = keyof Omit<BoardOutput, 'epics'>

const STATUS_TO_COLUMN: Record<string, Column> = {
  open: 'todo',
  ready: 'todo',
  in_progress: 'in_progress',
  blocked: 'in_progress',
  review: 'review',
  closed: 'done',
  archived: 'done',
  deferred: 'done',
}

const COLUMN_TO_STATUS: Record<Column, UpdatePatch['status']> = {
  todo: 'open',
  in_progress: 'in_progress',
  review: 'review',
  done: 'closed',
}

export function columnForStatus(status: string | null | undefined): Column | null {
  if (!status) return null
  return STATUS_TO_COLUMN[status] ?? null
}

export function statusForColumn(col: Column): NonNullable<UpdatePatch['status']> {
  return COLUMN_TO_STATUS[col]!
}

interface MoveCardVars {
  id: string
  /** Column the card was in before the drag — used to rollback on error. */
  from: Column
  /** Column the user dropped onto. */
  to: Column
}

/** Optimistic drag-to-column transition. */
export function useMoveCard(): UseMutationResult<UpdateOutputWire, unknown, MoveCardVars, { previous?: BoardOutput }> {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: ({ id, to }) =>
      updateTicket(id, { status: statusForColumn(to) }),
    onMutate: async ({ id, from, to }) => {
      await qc.cancelQueries({ queryKey: ['board'] })
      const previous = qc.getQueryData<BoardOutput>(['board', null, null])
      qc.setQueriesData<BoardOutput>({ queryKey: ['board'] }, (board) => {
        if (!board) return board
        const fromList = board[from]
        const card = fromList.find((c) => c.id === id)
        if (!card) return board
        return {
          ...board,
          [from]: fromList.filter((c) => c.id !== id),
          [to]: [card, ...board[to]],
        } as BoardOutput
      })
      return { previous }
    },
    onError: (err, _vars, ctx) => {
      if (ctx?.previous) {
        qc.setQueryData(['board', null, null], ctx.previous)
      }
      qc.invalidateQueries({ queryKey: ['board'] })
      toastApiError(err)
    },
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['board'] })
    },
  })
}

export function useCreateTicket(): UseMutationResult<CreatedTicketWire, unknown, CreateBody> {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (body) => createTicket(body),
    onSuccess: (out) => {
      // Optimistically prepend to the Todo column so the new card is visible
      // before the SSE echo lands.
      const env = out.record.envelope
      const card: BoardCard = {
        id: env.id,
        short_id: env.id.slice(0, 10),
        title: env.title,
        kind: env.kind,
        priority: env.priority,
        owner: env.owner?.name ?? null,
        epic_id: null,
        criteria_total: 0,
        criteria_met: 0,
        subtask_count: 0,
        blocked_by_count: 0,
      }
      qc.setQueriesData<BoardOutput>({ queryKey: ['board'] }, (board) => {
        if (!board) return board
        // Don't double-insert if a concurrent invalidate already pulled it in.
        if (board.todo.some((c) => c.id === card.id)) return board
        return { ...board, todo: [card, ...board.todo] } as BoardOutput
      })
      toast.success(`Created ${card.short_id}`)
      qc.invalidateQueries({ queryKey: ['board'] })
    },
    onError: (err) => toastApiError(err),
  })
}

export function useClaimTicket(id: string): UseMutationResult<ClaimOutputWire, unknown, void> {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: () => claimTicket(id),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ticketQueryKey(id) })
      qc.invalidateQueries({ queryKey: ['board'] })
      toast.success('Claimed')
    },
    onError: (err) => toastApiError(err),
  })
}

export function useUnclaimTicket(id: string): UseMutationResult<UnclaimOutputWire, unknown, void> {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: () => unclaimTicket(id),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ticketQueryKey(id) })
      qc.invalidateQueries({ queryKey: ['board'] })
      toast.success('Unclaimed')
    },
    onError: (err) => toastApiError(err),
  })
}

export function useCloseTicket(id: string): UseMutationResult<CloseOutputWire, unknown, { force?: boolean; reason?: string } | void> {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (body) => closeTicket(id, body || undefined),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ticketQueryKey(id) })
      qc.invalidateQueries({ queryKey: ['board'] })
      toast.success('Closed')
    },
    onError: (err) => toastApiError(err),
  })
}

export function useReopenTicket(id: string): UseMutationResult<CloseOutputWire, unknown, void> {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: () => reopenTicket(id),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ticketQueryKey(id) })
      qc.invalidateQueries({ queryKey: ['board'] })
      toast.success('Reopened')
    },
    onError: (err) => toastApiError(err),
  })
}

export function useUpdateTicket(id: string): UseMutationResult<UpdateOutputWire, unknown, UpdatePatch> {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (patch) => updateTicket(id, patch),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ticketQueryKey(id) })
      qc.invalidateQueries({ queryKey: ['board'] })
    },
    onError: (err) => toastApiError(err),
  })
}

export function useLinkTicket(id: string): UseMutationResult<unknown, unknown, LinkBody> {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (body) => linkTicket(id, body),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ticketQueryKey(id) })
      toast.success('Linked')
    },
    onError: (err) => toastApiError(err),
  })
}

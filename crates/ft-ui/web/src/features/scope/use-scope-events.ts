/**
 * SSE invalidator for the scope surface. Listens for `scope_*` events and
 * invalidates the scope-list / scope-aliases / per-scope caches. Mirrors the
 * shape of `features/tickets/use-ticket-events.ts`.
 */
import { useEffect } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import type { Event as ScopeEvent } from '@/api/types/Event'
import { useEvents } from '@/api/hooks/useEvents'
import { scopeListKey, scopeAliasesKey, scopePreviewKey } from './use-scope-query'

export function useScopeEvents(enabled = true) {
  const queryClient = useQueryClient()
  const { last } = useEvents<ScopeEvent>({ enabled })

  useEffect(() => {
    if (!last) return
    if (!last.kind.startsWith('scope_')) return
    queryClient.invalidateQueries({ queryKey: scopeListKey })
    queryClient.invalidateQueries({ queryKey: scopeAliasesKey })
    queryClient.invalidateQueries({ queryKey: scopePreviewKey })
    queryClient.invalidateQueries({ queryKey: ['scope'] })
  }, [last, queryClient])
}

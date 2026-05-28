/**
 * SSE invalidator for the audit surface. Listens for `lint_*` / `verify_*` /
 * `review_*` events to bust the relevant caches.
 */
import { useEffect } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import type { Event as AppEvent } from '@/api/types/Event'
import { useEvents } from '@/api/hooks/useEvents'

export function useAuditEvents(enabled = true) {
  const qc = useQueryClient()
  const { last } = useEvents<AppEvent>({ enabled })

  useEffect(() => {
    if (!last) return
    if (last.kind.startsWith('lint_')) {
      qc.invalidateQueries({ queryKey: ['audit-lint'] })
    }
    if (last.kind.startsWith('verify_')) {
      qc.invalidateQueries({ queryKey: ['audit-verify'] })
    }
    if (last.kind === 'review_approved' || last.kind === 'review_rejected') {
      qc.invalidateQueries({ queryKey: ['audit-review', last.id] })
      qc.invalidateQueries({ queryKey: ['audit-criteria', last.id] })
    }
  }, [last, qc])
}

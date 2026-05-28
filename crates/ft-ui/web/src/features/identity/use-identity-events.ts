import { useEffect } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import type { Event as IdentityEvent } from '@/api/types/Event'
import { useEvents } from '@/api/hooks/useEvents'
import { identityShowKey, identityCapsKey } from './use-identity-query'

export function useIdentityEvents(enabled = true) {
  const queryClient = useQueryClient()
  const { last } = useEvents<IdentityEvent>({ enabled })

  useEffect(() => {
    if (!last) return
    if (!last.kind.startsWith('identity_')) return
    queryClient.invalidateQueries({ queryKey: ['identity-list'] })
    if (last.kind === 'identity_updated') {
      queryClient.invalidateQueries({ queryKey: identityShowKey(last.identity) })
      queryClient.invalidateQueries({ queryKey: identityCapsKey(last.identity) })
    }
  }, [last, queryClient])
}

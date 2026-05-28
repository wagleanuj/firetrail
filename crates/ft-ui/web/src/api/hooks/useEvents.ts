import { useEffect, useRef, useState } from 'react'
import { isOwnRequestId } from '@/lib/request-id'

/**
 * Wire-level envelope mirroring `ft_ops::EmittedEvent`. Once `cargo xtask
 * gen-ts` has run, this can be replaced by the generated type.
 */
export interface EmittedEvent<E = unknown> {
  request_id: string | null
  event: E
}

export interface UseEventsOptions {
  /** Override the SSE endpoint. Defaults to `/api/events`. */
  url?: string
  /** Disable subscription (useful in tests). */
  enabled?: boolean
}

export interface UseEventsResult<E> {
  /** Most recent event seen. */
  last: E | null
  /** Connection readiness. */
  state: 'connecting' | 'open' | 'closed'
}

/**
 * Subscribe to the ft-ui SSE event stream.
 *
 * Filters out events whose `request_id` matches an id minted by this client,
 * so optimistic UI updates aren't double-applied.
 */
export function useEvents<E = unknown>(opts: UseEventsOptions = {}): UseEventsResult<E> {
  const { url = '/api/events', enabled = true } = opts
  const [last, setLast] = useState<E | null>(null)
  const [state, setState] = useState<UseEventsResult<E>['state']>('connecting')
  const sourceRef = useRef<EventSource | null>(null)

  useEffect(() => {
    if (!enabled) {
      setState('closed')
      return
    }
    const es = new EventSource(url, { withCredentials: true })
    sourceRef.current = es
    setState('connecting')

    es.onopen = () => setState('open')
    es.onerror = () => setState('closed')
    es.onmessage = (msg) => {
      try {
        const envelope = JSON.parse(msg.data) as EmittedEvent<E>
        if (isOwnRequestId(envelope.request_id)) return
        setLast(envelope.event)
      } catch {
        // Ignore malformed frames; the server is the source of truth.
      }
    }

    return () => {
      es.close()
      sourceRef.current = null
      setState('closed')
    }
  }, [url, enabled])

  return { last, state }
}

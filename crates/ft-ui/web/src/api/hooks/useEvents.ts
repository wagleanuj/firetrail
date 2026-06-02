import { useEffect, useState } from 'react'
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

type ConnState = UseEventsResult<unknown>['state']
type EventListener = (event: unknown) => void
type StateListener = (state: ConnState) => void

/**
 * One EventSource for the whole app, shared by every {@link useEvents} consumer.
 *
 * SSE streams are long-lived HTTP/1.1 requests. Browsers cap concurrent
 * connections per origin at six, so opening a stream per hook (AppShell alone
 * mounts two) burns through that budget and eventually starves ordinary
 * `fetch()`s — search would spin forever with the server idle. A single
 * ref-counted source keeps the app at exactly one connection regardless of how
 * many components subscribe.
 */
const eventListeners = new Set<EventListener>()
const stateListeners = new Set<StateListener>()
let source: EventSource | null = null
let connState: ConnState = 'closed'

function publishState(next: ConnState): void {
  connState = next
  for (const listener of stateListeners) listener(next)
}

function ensureSource(url: string): void {
  if (source) return
  const es = new EventSource(url, { withCredentials: true })
  source = es
  publishState('connecting')

  es.onopen = () => publishState('open')
  es.onerror = () => publishState('closed')
  es.onmessage = (msg) => {
    try {
      const envelope = JSON.parse(msg.data) as EmittedEvent
      // Filtered once for all consumers: a client's own optimistic mutation
      // shouldn't echo back and clobber its in-flight update.
      if (isOwnRequestId(envelope.request_id)) return
      for (const listener of eventListeners) listener(envelope.event)
    } catch {
      // Ignore malformed frames; the server is the source of truth.
    }
  }
}

function releaseIfIdle(): void {
  if (source && eventListeners.size === 0 && stateListeners.size === 0) {
    source.close()
    source = null
    connState = 'closed'
  }
}

/**
 * Subscribe to the ft-ui SSE event stream.
 *
 * Every consumer shares a single underlying connection (see the manager above).
 * Filters out events whose `request_id` matches an id minted by this client,
 * so optimistic UI updates aren't double-applied.
 */
export function useEvents<E = unknown>(opts: UseEventsOptions = {}): UseEventsResult<E> {
  const { url = '/api/events', enabled = true } = opts
  const [last, setLast] = useState<E | null>(null)
  const [state, setState] = useState<ConnState>('connecting')

  useEffect(() => {
    if (!enabled) {
      setState('closed')
      return
    }

    const onEvent: EventListener = (event) => setLast(event as E)
    const onState: StateListener = (next) => setState(next)
    eventListeners.add(onEvent)
    stateListeners.add(onState)

    ensureSource(url)
    setState(connState)

    return () => {
      eventListeners.delete(onEvent)
      stateListeners.delete(onState)
      releaseIfIdle()
    }
  }, [url, enabled])

  return { last, state }
}

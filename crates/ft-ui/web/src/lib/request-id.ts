/**
 * Generates a client-side request id used to coalesce optimistic mutations
 * with SSE events emitted by ft-ui. The frontend should attach the id to
 * outgoing mutations via the `X-Firetrail-Request-Id` header, and the SSE
 * consumer should drop any `EmittedEvent` whose `request_id` matches an id
 * that this client recently sent (the local mutation already updated state).
 */
let counter = 0

export function newRequestId(): string {
  counter = (counter + 1) >>> 0
  const rnd =
    typeof crypto !== 'undefined' && 'randomUUID' in crypto
      ? crypto.randomUUID().slice(0, 8)
      : Math.random().toString(16).slice(2, 10)
  return `c_${Date.now().toString(36)}_${counter.toString(36)}_${rnd}`
}

/** In-memory registry of request ids this client originated. */
const owned = new Set<string>()
const MAX = 512

export function trackRequestId(id: string): void {
  owned.add(id)
  if (owned.size > MAX) {
    const first = owned.values().next().value
    if (first) owned.delete(first)
  }
}

export function isOwnRequestId(id: string | null | undefined): boolean {
  return !!id && owned.has(id)
}

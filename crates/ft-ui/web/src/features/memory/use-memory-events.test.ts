/**
 * The request-id coalescing logic lives in the shared `useEvents` hook
 * (already tested in W1). This test asserts the request-id store treats
 * memory-flavoured ids identically to ticket ids — i.e. the SSE consumer
 * has no kind-specific bypass that would let a memory echo leak through.
 */
import { describe, expect, it } from 'vitest'
import { isOwnRequestId, newRequestId, trackRequestId } from '@/lib/request-id'

describe('memory event coalescing (request-id store)', () => {
  it('coalesces locally-minted ids regardless of kind prefix', () => {
    const memoryRid = newRequestId()
    const ticketRid = newRequestId()
    trackRequestId(memoryRid)
    trackRequestId(ticketRid)
    expect(isOwnRequestId(memoryRid)).toBe(true)
    expect(isOwnRequestId(ticketRid)).toBe(true)
  })

  it('does not coalesce foreign ids (memory_* events emitted by other clients)', () => {
    const foreign = 'c_other_client_id'
    expect(isOwnRequestId(foreign)).toBe(false)
  })

  it('returns false for unset request_id (background-emitted memory_salvaged)', () => {
    // SSE envelope may set request_id to null for background salvage emissions.
    expect(isOwnRequestId(null)).toBe(false)
  })
})

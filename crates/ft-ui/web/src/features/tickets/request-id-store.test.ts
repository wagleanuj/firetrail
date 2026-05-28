import { describe, expect, it } from 'vitest'
import { isOwnRequestId, newRequestId, trackRequestId } from '@/lib/request-id'

describe('request-id store', () => {
  it('mints unique ids', () => {
    const a = newRequestId()
    const b = newRequestId()
    expect(a).not.toBe(b)
    expect(a).toMatch(/^c_/)
  })

  it('tracks ids and reports membership', () => {
    const id = newRequestId()
    expect(isOwnRequestId(id)).toBe(false)
    trackRequestId(id)
    expect(isOwnRequestId(id)).toBe(true)
  })

  it('returns false for null/undefined/unknown ids', () => {
    expect(isOwnRequestId(null)).toBe(false)
    expect(isOwnRequestId(undefined)).toBe(false)
    expect(isOwnRequestId('never-tracked-id')).toBe(false)
  })

  it('caps the in-memory registry to bound memory', () => {
    // Spam past the MAX (512) — the oldest entries should evict.
    const first = newRequestId()
    trackRequestId(first)
    for (let i = 0; i < 600; i++) trackRequestId(newRequestId())
    expect(isOwnRequestId(first)).toBe(false)
  })
})

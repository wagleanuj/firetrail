import { describe, it, expect } from 'vitest'
import { epicColor } from './epic-color'

describe('epicColor', () => {
  it('is deterministic for the same id', () => {
    expect(epicColor('EPIC-abc')).toBe(epicColor('EPIC-abc'))
  })
  it('returns an hsl string', () => {
    expect(epicColor('EPIC-abc')).toMatch(/^hsl\(/)
  })
  it('differs for different ids (usually)', () => {
    // not a hard guarantee, but two distinct sample ids should differ
    expect(epicColor('EPIC-aaaa')).not.toBe(epicColor('EPIC-zzzz'))
  })
})

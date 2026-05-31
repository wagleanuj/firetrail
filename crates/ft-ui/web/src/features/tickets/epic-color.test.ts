import { describe, it, expect } from 'vitest'
import { epicColor, epicColorSoft } from './epic-color'

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

describe('epicColorSoft', () => {
  it('is deterministic for the same id', () => {
    expect(epicColorSoft('EPIC-abc')).toBe(epicColorSoft('EPIC-abc'))
  })
  it('returns a valid hsl with alpha', () => {
    expect(epicColorSoft('EPIC-abc')).toMatch(/^hsl\(.*\/ 0\.13\)$/)
  })
  it('uses the same hue as epicColor for the same id', () => {
    const solid = epicColor('EPIC-abc')
    const soft = epicColorSoft('EPIC-abc')
    // extract hue from both: "hsl(<hue> ..."
    const hueFrom = (s: string) => s.match(/^hsl\((\d+)/)![1]
    expect(hueFrom(soft)).toBe(hueFrom(solid))
  })
})

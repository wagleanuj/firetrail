import { describe, expect, it } from 'vitest'
import { MEMORY_SCHEMAS } from './schemas'

describe('memory schemas', () => {
  it('accepts a valid incident', () => {
    const r = MEMORY_SCHEMAS.incident.safeParse({ summary: 'API down', services: 'api' })
    expect(r.success).toBe(true)
    if (r.success) expect(r.data.services).toEqual(['api'])
  })
  it('rejects empty incident summary', () => {
    const r = MEMORY_SCHEMAS.incident.safeParse({ summary: '' })
    expect(r.success).toBe(false)
  })

  it('accepts a valid finding', () => {
    const r = MEMORY_SCHEMAS.finding.safeParse({
      summary: 'leaky pool',
      affected: 'src/a.rs, src/b.rs',
    })
    expect(r.success).toBe(true)
    if (r.success) expect(r.data.affected).toEqual(['src/a.rs', 'src/b.rs'])
  })
  it('rejects empty finding summary', () => {
    expect(MEMORY_SCHEMAS.finding.safeParse({ summary: '' }).success).toBe(false)
  })

  it('accepts a valid runbook', () => {
    const r = MEMORY_SCHEMAS.runbook.safeParse({
      title: 'Restart api',
      summary: 'When the api hangs.',
      appliesTo: 'api',
    })
    expect(r.success).toBe(true)
  })
  it('rejects empty runbook title', () => {
    expect(
      MEMORY_SCHEMAS.runbook.safeParse({ title: '', summary: 'x' }).success,
    ).toBe(false)
  })

  it('accepts a valid decision', () => {
    expect(
      MEMORY_SCHEMAS.decision.safeParse({
        title: 'ADR',
        context: 'why',
        decision: 'what',
      }).success,
    ).toBe(true)
  })
  it('rejects empty decision title', () => {
    expect(
      MEMORY_SCHEMAS.decision.safeParse({ title: '', context: 'x', decision: 'y' }).success,
    ).toBe(false)
  })

  it('accepts a valid gotcha', () => {
    expect(
      MEMORY_SCHEMAS.gotcha.safeParse({ summary: 'mind the cache' }).success,
    ).toBe(true)
  })
  it('rejects empty gotcha summary', () => {
    expect(MEMORY_SCHEMAS.gotcha.safeParse({ summary: '' }).success).toBe(false)
  })

  it('accepts a valid memory', () => {
    const r = MEMORY_SCHEMAS.memory.safeParse({
      title: 'hello',
      body: 'world',
      tags: 'a, b',
    })
    expect(r.success).toBe(true)
    if (r.success) expect(r.data.tags).toEqual(['a', 'b'])
  })
  it('rejects empty memory title', () => {
    expect(
      MEMORY_SCHEMAS.memory.safeParse({ title: '', body: 'x' }).success,
    ).toBe(false)
  })
})

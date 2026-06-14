import { describe, expect, it } from 'vitest'
import { recordIdFromAuditDoc, resultTarget } from './result-nav'

const TASK = `TASK-${'a'.repeat(64)}`
const MEM = `MEM-${'b'.repeat(64)}`

describe('recordIdFromAuditDoc', () => {
  it('strips the audit: tag and #h<n> suffix', () => {
    expect(recordIdFromAuditDoc(`audit:${TASK}#h0`)).toBe(TASK)
    expect(recordIdFromAuditDoc(`audit:${TASK}#h12`)).toBe(TASK)
  })

  it('tolerates a key without a #h<n> suffix', () => {
    expect(recordIdFromAuditDoc(`audit:${TASK}`)).toBe(TASK)
  })

  it('returns null for non-audit ids and empty record portions', () => {
    expect(recordIdFromAuditDoc(TASK)).toBeNull()
    expect(recordIdFromAuditDoc('scope:apps/checkout')).toBeNull()
    expect(recordIdFromAuditDoc('audit:')).toBeNull()
    expect(recordIdFromAuditDoc('audit:#h0')).toBeNull()
  })
})

describe('resultTarget', () => {
  it('routes ticket kinds to /tickets/$id', () => {
    for (const kind of ['epic', 'task', 'subtask', 'bug']) {
      expect(resultTarget(kind, TASK)).toEqual({ to: '/tickets/$id', params: { id: TASK } })
    }
  })

  it('routes memory kinds to /memory/$id', () => {
    for (const kind of [
      'incident',
      'finding',
      'runbook',
      'decision',
      'gotcha',
      'memory',
      'doc',
      'repo_profile',
    ]) {
      expect(resultTarget(kind, MEM)).toEqual({ to: '/memory/$id', params: { id: MEM } })
    }
  })

  it('routes scope/identity synthetic docs to their pages with the bare key', () => {
    expect(resultTarget('scope', 'scope:apps/checkout')).toEqual({
      to: '/scope/$id',
      params: { id: 'apps/checkout' },
    })
    expect(resultTarget('identity', 'identity:alice')).toEqual({
      to: '/identity/$id',
      params: { id: 'alice' },
    })
  })

  it('resolves an audit hit to its underlying record (firetrail-g5n6)', () => {
    // audit echo of a task -> the task, on the tickets surface (NOT /memory)
    expect(resultTarget('audit', `audit:${TASK}#h0`)).toEqual({
      to: '/tickets/$id',
      params: { id: TASK },
    })
    // audit echo of a memory record -> the memory detail route
    expect(resultTarget('audit', `audit:${MEM}#h3`)).toEqual({
      to: '/memory/$id',
      params: { id: MEM },
    })
  })

  it('returns null for unknown kinds and unlinkable audit keys', () => {
    expect(resultTarget('mystery', 'whatever')).toBeNull()
    expect(resultTarget('audit', 'audit:')).toBeNull()
  })
})

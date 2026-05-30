/**
 * Unit tests for the ticket-detail relation filtering (firetrail-e4jv).
 */
import { describe, it, expect } from 'vitest'
import type { Relation } from '@/api/wire/relation'
import type { TicketRelationKind } from '@/api/types/TicketRelationKind'
import { visibleRelations } from './ticket-detail'

function rel(kind: string): Relation {
  return {
    from: 'task:a',
    to: 'task:b',
    kind: kind as TicketRelationKind,
    created_at: '2026-05-30T00:00:00Z',
    created_by: { id: 'id:1', name: 'tester' },
  }
}

describe('visibleRelations', () => {
  it('hides documented-in edges (they live in the Docs panel)', () => {
    const input = [rel('blocks'), rel('documented-in'), rel('related-to')]
    const out = visibleRelations(input)
    expect(out.map((r) => r.kind)).toEqual(['blocks', 'related-to'])
  })

  it('passes through when there are no doc edges', () => {
    const input = [rel('blocks'), rel('child-of')]
    expect(visibleRelations(input)).toHaveLength(2)
  })

  it('returns empty when every relation is a doc edge', () => {
    expect(visibleRelations([rel('documented-in')])).toEqual([])
  })
})

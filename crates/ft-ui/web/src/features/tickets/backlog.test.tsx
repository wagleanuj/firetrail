import { describe, it, expect } from 'vitest'
import { sortRows, type BacklogRow } from './backlog'

const rows: BacklogRow[] = [
  { id: 'A', short_id: 'TASK-A', kind: 'task', title: 'a', priority: 'p3', status: 'todo', epic_id: null, owner: null, criteria_total: 0, criteria_met: 0 } as BacklogRow,
  { id: 'B', short_id: 'BUG-B', kind: 'bug', title: 'b', priority: 'p0', status: 'todo', epic_id: null, owner: null, criteria_total: 0, criteria_met: 0 } as BacklogRow,
]

describe('sortRows', () => {
  it('sorts by priority ascending (p0 first) then descending', () => {
    expect(sortRows(rows, 'priority', 'asc').map((r) => r.id)).toEqual(['B', 'A'])
    expect(sortRows(rows, 'priority', 'desc').map((r) => r.id)).toEqual(['A', 'B'])
  })
  it('sorts by title', () => {
    expect(sortRows(rows, 'title', 'asc').map((r) => r.id)).toEqual(['A', 'B'])
  })
})

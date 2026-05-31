import { describe, it, expect } from 'vitest'
import { groupByEpic } from './board-swimlanes'
import type { BoardOutput } from '@/api/types/BoardOutput'

function makeCard(id: string, epic_id: string | null) {
  return { id, short_id: id, title: id, kind: 'task', priority: 'p2', owner: null, epic_id, criteria_total: 0, criteria_met: 0, subtask_count: 0, blocked_by_count: 0 }
}

describe('groupByEpic', () => {
  it('groups cards into epic lanes + a no-epic lane', () => {
    const out: BoardOutput = {
      todo: [makeCard('T1', 'E1'), makeCard('T2', null)],
      in_progress: [],
      review: [],
      done: [],
      epics: [{ id: 'E1', short_id: 'E1', title: 'Auth' }],
    }
    const lanes = groupByEpic(out)
    expect(lanes.map((l) => l.key)).toEqual(['E1', ''])
    expect(lanes[0].columns.todo).toHaveLength(1)
    expect(lanes[1].columns.todo).toHaveLength(1)
  })

  it('omits the no-epic lane when every card has an epic', () => {
    const out: BoardOutput = {
      todo: [makeCard('T1', 'E1')],
      in_progress: [],
      review: [],
      done: [],
      epics: [{ id: 'E1', short_id: 'E1', title: 'Auth' }],
    }
    expect(groupByEpic(out).map((l) => l.key)).toEqual(['E1'])
  })

  it('places a card with an unknown epic_id in the no-epic lane, no card is lost', () => {
    const out: BoardOutput = {
      todo: [makeCard('T1', 'E1'), makeCard('T2', 'E_GONE')],
      in_progress: [makeCard('T3', null)],
      review: [],
      done: [],
      epics: [{ id: 'E1', short_id: 'E1', title: 'Auth' }],
    }
    const lanes = groupByEpic(out)

    // E1 lane exists; no-epic lane exists (key '')
    expect(lanes.map((l) => l.key)).toEqual(['E1', ''])

    // T2 (unknown epic) and T3 (null epic) both land in the no-epic lane
    const noEpicLane = lanes.find((l) => l.key === '')!
    expect(noEpicLane.columns.todo).toHaveLength(1)       // T2
    expect(noEpicLane.columns.in_progress).toHaveLength(1) // T3

    // Total cards across all lanes must equal input card count (3)
    const totalAcrossLanes = lanes.reduce(
      (sum, l) =>
        sum +
        l.columns.todo.length +
        l.columns.in_progress.length +
        l.columns.review.length +
        l.columns.done.length,
      0,
    )
    expect(totalAcrossLanes).toBe(3)
  })
})

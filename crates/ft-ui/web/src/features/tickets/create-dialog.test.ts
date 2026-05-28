import { describe, expect, it } from 'vitest'
import { SCHEMAS } from './create-dialog'

describe('create-dialog schemas', () => {
  it('rejects empty task title', () => {
    const r = SCHEMAS.task.safeParse({ title: '', labels: '' })
    expect(r.success).toBe(false)
  })

  it('accepts a minimal task', () => {
    const r = SCHEMAS.task.safeParse({ title: 'Do the thing', labels: '' })
    expect(r.success).toBe(true)
    if (r.success) expect(r.data.title).toBe('Do the thing')
  })

  it('accepts comma-separated key=value labels', () => {
    const r = SCHEMAS.task.safeParse({ title: 'x', labels: 'a=1, b=2' })
    expect(r.success).toBe(true)
  })

  it('rejects malformed labels (missing =)', () => {
    const r = SCHEMAS.task.safeParse({ title: 'x', labels: 'oops' })
    expect(r.success).toBe(false)
  })

  it('requires a parent on subtask', () => {
    const r = SCHEMAS.subtask.safeParse({ title: 'x', parent: '', labels: '' })
    expect(r.success).toBe(false)
  })

  it('parses a minimal bug', () => {
    const r = SCHEMAS.bug.safeParse({ title: 'broken', labels: '' })
    expect(r.success).toBe(true)
  })
})

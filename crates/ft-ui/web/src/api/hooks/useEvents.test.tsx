import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import { renderHook, act } from '@testing-library/react'
import { useEvents } from './useEvents'

/**
 * jsdom has no EventSource. This fake records every instance so we can assert
 * how many real connections the app opens, fan a message out to the handler
 * the hook installed, and observe close()/refcount behaviour.
 */
class FakeEventSource {
  static instances: FakeEventSource[] = []
  static openCount = 0

  url: string
  withCredentials: boolean
  onmessage: ((ev: MessageEvent) => void) | null = null
  onopen: (() => void) | null = null
  onerror: (() => void) | null = null
  closed = false

  constructor(url: string, init?: { withCredentials?: boolean }) {
    this.url = url
    this.withCredentials = init?.withCredentials ?? false
    FakeEventSource.instances.push(this)
    FakeEventSource.openCount += 1
  }

  /** Deliver a server frame to whatever handler the hook installed. */
  emit(data: unknown) {
    this.onmessage?.({ data: JSON.stringify(data) } as MessageEvent)
  }

  close() {
    this.closed = true
  }
}

beforeEach(() => {
  FakeEventSource.instances = []
  FakeEventSource.openCount = 0
  ;(globalThis as { EventSource?: unknown }).EventSource = FakeEventSource
})

afterEach(() => {
  // Auto-cleanup unmounts the hooks; nothing else to tear down.
})

describe('useEvents shared connection', () => {
  it('opens exactly one EventSource across multiple concurrent consumers', () => {
    const a = renderHook(() => useEvents())
    const b = renderHook(() => useEvents())
    const c = renderHook(() => useEvents())

    expect(FakeEventSource.openCount).toBe(1)

    a.unmount()
    b.unmount()
    c.unmount()
  })

  it('fans a single server event out to every consumer', () => {
    const a = renderHook(() => useEvents<{ kind: string }>())
    const b = renderHook(() => useEvents<{ kind: string }>())

    expect(FakeEventSource.instances).toHaveLength(1)
    const source = FakeEventSource.instances[0]

    act(() => {
      source.emit({ request_id: null, event: { kind: 'memory_written' } })
    })

    expect(a.result.current.last).toEqual({ kind: 'memory_written' })
    expect(b.result.current.last).toEqual({ kind: 'memory_written' })

    a.unmount()
    b.unmount()
  })

  it('keeps the connection open until the last consumer unmounts', () => {
    const a = renderHook(() => useEvents())
    const b = renderHook(() => useEvents())
    const source = FakeEventSource.instances[0]

    a.unmount()
    expect(source.closed).toBe(false) // b is still subscribed

    b.unmount()
    expect(source.closed).toBe(true) // refcount hit zero
  })
})

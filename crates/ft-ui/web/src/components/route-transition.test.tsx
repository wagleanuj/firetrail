import { describe, expect, it } from 'vitest'
import { sectionFor } from './route-transition'

/**
 * `sectionFor` is the key that drives the root `<AnimatePresence>`. Two routes
 * that resolve to the *same* section share a key, so navigating between them
 * does NOT trigger an exit/enter fade or unmount the shared subtree. The board
 * (`/`) and the ticket drawer (`/tickets/:id`) MUST collapse to one section so
 * opening a ticket slides the drawer over a persistent board instead of
 * flashing the whole page.
 */
describe('sectionFor', () => {
  it('collapses the board index and the ticket drawer to one section', () => {
    // Both routeId shapes the generated tree can produce for the board layout.
    expect(sectionFor('/_board/')).toBe('board')
    expect(sectionFor('/_board/tickets/$id')).toBe('board')
    // Defensive: bare paths map there too.
    expect(sectionFor('/')).toBe('board')
    expect(sectionFor('/tickets/$id')).toBe('board')
  })

  it('opening a ticket does not change the transition key', () => {
    // The regression this fix targets: clicking a card used to swap the key
    // (/ -> /tickets/$id), fading the board to blank and remounting it.
    expect(sectionFor('/_board/')).toBe(sectionFor('/_board/tickets/$id'))
  })

  it('keeps genuinely different sections distinct so cross-section nav still fades', () => {
    expect(sectionFor('/memory/')).toBe('memory')
    expect(sectionFor('/memory/$id')).toBe('memory')
    expect(sectionFor('/audit/diff')).toBe('audit')
    expect(sectionFor('/identity/')).toBe('identity')
    expect(sectionFor('/scope/$id')).toBe('scope')
    // …and these are all different from the board section.
    const sections = new Set([
      sectionFor('/_board/'),
      sectionFor('/memory/'),
      sectionFor('/audit/diff'),
      sectionFor('/identity/'),
      sectionFor('/scope/$id'),
    ])
    expect(sections.size).toBe(5)
  })

  it('falls back to a stable key when there is no match', () => {
    expect(sectionFor(undefined)).toBe('root')
  })
})

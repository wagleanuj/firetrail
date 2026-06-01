/**
 * Wraps the router `<Outlet>` in a short fade so route changes feel less
 * jarring. Keyed on the top-level *section* (not the exact route) so that
 * navigating *within* a section — e.g. opening the ticket drawer at
 * `/tickets/:id` over the board at `/` — does NOT re-trigger the fade or
 * unmount the shared view. Cross-section moves (board → memory → audit) still
 * fade. Honours `prefers-reduced-motion`.
 */
import { Outlet, useRouterState } from '@tanstack/react-router'
import { AnimatePresence, motion, useReducedMotion } from 'framer-motion'
import { ROUTE_TRANSITION, reducedTransition } from '@/lib/motion'

/**
 * Collapse a route id to the section that owns a persistent layout. The board
 * (`/`) and the ticket drawer (`/tickets/:id`) share the `_board` layout, so
 * both map to `board`; everything else groups by its first path segment.
 */
export function sectionFor(routeId: string | undefined): string {
  if (!routeId) return 'root'
  if (routeId === '/' || routeId.includes('_board') || routeId.startsWith('/tickets')) {
    return 'board'
  }
  const seg = routeId.split('/').filter(Boolean)[0]
  return seg ?? 'root'
}

export function RouteTransition() {
  const reduced = useReducedMotion() ?? false
  const matches = useRouterState({ select: (s) => s.matches })
  const lastMatch = matches[matches.length - 1]
  const key = sectionFor(lastMatch?.routeId)
  const transition = reducedTransition(reduced, ROUTE_TRANSITION)
  return (
    <AnimatePresence mode="wait" initial={false}>
      <motion.div
        key={key}
        initial={{ opacity: 0, y: reduced ? 0 : 4 }}
        animate={{ opacity: 1, y: 0 }}
        exit={{ opacity: 0, y: reduced ? 0 : -4 }}
        transition={transition}
        className="h-full"
      >
        <Outlet />
      </motion.div>
    </AnimatePresence>
  )
}

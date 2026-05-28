/**
 * Wraps the router `<Outlet>` in a short fade so route changes feel less
 * jarring. Keyed on the matched route path so `<AnimatePresence>` triggers
 * exit + enter on navigation. Honours `prefers-reduced-motion`.
 */
import { Outlet, useRouterState } from '@tanstack/react-router'
import { AnimatePresence, motion, useReducedMotion } from 'framer-motion'
import { ROUTE_TRANSITION, reducedTransition } from '@/lib/motion'

export function RouteTransition() {
  const reduced = useReducedMotion() ?? false
  const matches = useRouterState({ select: (s) => s.matches })
  const lastMatch = matches[matches.length - 1]
  const key = lastMatch?.routeId ?? 'root'
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

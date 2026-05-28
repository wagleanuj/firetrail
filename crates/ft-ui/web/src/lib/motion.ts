/**
 * Shared motion primitives. Centralised so we can tune timings and respect
 * `prefers-reduced-motion` from a single place.
 *
 * All durations are short — the goal is "feels polished" rather than "feels
 * like a slideshow". When the user prefers reduced motion, every transition
 * collapses to an instant cut (duration: 0).
 */
import type { Transition } from 'framer-motion'

export const FADE_DURATION = 0.18

/** Soft ease-out used by route transitions. */
export const ROUTE_TRANSITION: Transition = {
  duration: FADE_DURATION,
  ease: [0.16, 1, 0.3, 1],
}

/** Layout transition for kanban cards. */
export const LAYOUT_TRANSITION: Transition = {
  type: 'spring',
  stiffness: 420,
  damping: 38,
  mass: 0.8,
}

/** Per-item stagger delay used by mounting lists. */
export const LIST_STAGGER = 0.03

export function reducedTransition(reduced: boolean, base: Transition): Transition {
  return reduced ? { duration: 0 } : base
}

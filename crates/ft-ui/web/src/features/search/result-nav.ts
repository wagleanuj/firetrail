/**
 * Map a global-search hit (`kind` + storage `id`) onto a TanStack Router
 * navigation target, plus a Badge variant for the kind chip.
 *
 * Storage id shapes (from `ft_search::DocId::as_storage_str`):
 *   - records        → bare record id (`TASK-…`, `MEM-…`)
 *   - scope synthetic → `scope:<scope-id>`
 *   - identity synth. → `identity:<identity-id>`
 *   - audit synthetic → `audit:<record-id>#h<n>`
 */
import type { BadgeProps } from '@/components/ui/badge'

const WORK_KINDS = new Set(['epic', 'task', 'subtask', 'bug'])
const MEMORY_KINDS = new Set([
  'incident',
  'finding',
  'runbook',
  'decision',
  'gotcha',
  'memory',
  'doc',
])

/** A router `navigate` target for a hit, or `null` when it isn't routable. */
export interface ResultTarget {
  to: string
  params?: Record<string, string>
}

/** Strip the `<tag>:` prefix from a synthetic doc id, returning the key. */
function syntheticKey(id: string): string {
  const idx = id.indexOf(':')
  return idx === -1 ? id : id.slice(idx + 1)
}

/**
 * Resolve where selecting a hit should navigate. Audit entries route to the
 * audit dashboard (there is no per-entry detail route); unknown kinds return
 * `null` so the caller can no-op rather than navigate somewhere wrong.
 */
export function resultTarget(kind: string, id: string): ResultTarget | null {
  if (WORK_KINDS.has(kind)) return { to: '/tickets/$id', params: { id } }
  if (MEMORY_KINDS.has(kind)) return { to: '/memory/$id', params: { id } }
  if (kind === 'scope') return { to: '/scope/$id', params: { id: syntheticKey(id) } }
  if (kind === 'identity') return { to: '/identity/$id', params: { id: syntheticKey(id) } }
  if (kind === 'audit') return { to: '/audit' }
  return null
}

/**
 * Pick the design-system Badge variant for a kind. The redesign Badge ships
 * `feature | bug | task | epic` type variants; everything else falls back to
 * `secondary` so memory/synthetic kinds still render a readable chip.
 */
export function kindBadgeVariant(kind: string): BadgeProps['variant'] {
  switch (kind) {
    case 'epic':
      return 'epic'
    case 'task':
    case 'subtask':
      return 'task'
    case 'bug':
    case 'incident':
      return 'bug'
    case 'finding':
    case 'runbook':
    case 'decision':
    case 'gotcha':
    case 'memory':
    case 'doc':
      return 'feature'
    default:
      return 'secondary'
  }
}

/** All filterable kinds, grouped for the palette's filter chips. */
export const SEARCH_KIND_CHIPS = [
  'task',
  'bug',
  'epic',
  'memory',
  'gotcha',
  'decision',
  'runbook',
  'finding',
  'incident',
  'doc',
  'scope',
  'identity',
  'audit',
] as const

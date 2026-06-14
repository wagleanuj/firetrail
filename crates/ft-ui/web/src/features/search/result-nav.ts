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
  'repo_profile',
])

/** Uppercase `RecordId` prefixes (ADR-0015) for ticket-surface kinds. */
const WORK_ID_PREFIXES = ['TASK-', 'EPIC-', 'SUB-', 'BUG-']

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
 * Underlying `RecordId` from an `audit:<RecordId>#h<n>` synthetic doc id.
 * Returns `null` for non-audit ids or an empty record portion.
 */
export function recordIdFromAuditDoc(id: string): string | null {
  const tag = 'audit:'
  if (!id.startsWith(tag)) return null
  const rest = id.slice(tag.length)
  const hash = rest.indexOf('#')
  const recordId = hash === -1 ? rest : rest.slice(0, hash)
  return recordId.length > 0 ? recordId : null
}

/** Route a real record by its id prefix: ticket surface vs memory surface. */
function recordTarget(recordId: string): ResultTarget {
  const isWork = WORK_ID_PREFIXES.some((p) => recordId.startsWith(p))
  return isWork
    ? { to: '/tickets/$id', params: { id: recordId } }
    : { to: '/memory/$id', params: { id: recordId } }
}

/**
 * Resolve where selecting a hit should navigate.
 *
 * Audit entries are per-history-entry echoes (`audit:<RecordId>#h<n>`) with no
 * detail route of their own, so they resolve to their *underlying* record's
 * detail page. Unknown kinds (or a malformed audit key) return `null` so the
 * caller can no-op rather than navigate somewhere wrong.
 */
export function resultTarget(kind: string, id: string): ResultTarget | null {
  if (WORK_KINDS.has(kind)) return { to: '/tickets/$id', params: { id } }
  if (MEMORY_KINDS.has(kind)) return { to: '/memory/$id', params: { id } }
  if (kind === 'scope') return { to: '/scope/$id', params: { id: syntheticKey(id) } }
  if (kind === 'identity') return { to: '/identity/$id', params: { id: syntheticKey(id) } }
  if (kind === 'audit') {
    const recordId = recordIdFromAuditDoc(id)
    return recordId ? recordTarget(recordId) : null
  }
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

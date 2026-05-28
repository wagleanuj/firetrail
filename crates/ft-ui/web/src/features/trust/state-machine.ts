/**
 * Valid trust transitions per source state. Mirrors `ft-trust::TrustState`
 * and the ADR-0013 invariants — keeping the list here lets the UI render
 * only the actions the backend will accept.
 */
import type { TrustStateInput } from '@/api/types/TrustStateInput'

export type TrustOp =
  | 'review'
  | 'promote'
  | 'deprecate'
  | 'archive'
  | 'supersede'
  | 'redact'
  | 'merge'

export const ALL_OPS: TrustOp[] = [
  'review',
  'promote',
  'deprecate',
  'archive',
  'supersede',
  'redact',
  'merge',
]

const TRANSITIONS: Record<TrustStateInput, TrustOp[]> = {
  draft: ['review', 'archive', 'supersede', 'redact'],
  reviewed: ['promote', 'deprecate', 'archive', 'supersede', 'redact', 'merge'],
  verified: ['deprecate', 'archive', 'supersede', 'redact', 'merge'],
  stale: ['review', 'deprecate', 'archive', 'supersede', 'redact'],
  deprecated: ['archive', 'redact'],
  archived: ['redact'],
  superseded: ['redact'],
  rejected: ['archive', 'redact'],
  redacted: [],
}

export function validOps(state: string | null | undefined): TrustOp[] {
  if (!state) return []
  const ops = TRANSITIONS[state as TrustStateInput]
  return ops ?? []
}

export const OP_LABELS: Record<TrustOp, string> = {
  review: 'Review',
  promote: 'Promote',
  deprecate: 'Deprecate',
  archive: 'Archive',
  supersede: 'Supersede',
  redact: 'Redact',
  merge: 'Merge',
}

/** Risk classes that demand evidence on promote (per ADR-0013). */
const HIGH_STAKES = new Set(['security', 'incident-root-cause', 'pii', 'compliance'])

export function isHighStakes(riskClass: string | null | undefined): boolean {
  if (!riskClass) return false
  return HIGH_STAKES.has(riskClass)
}

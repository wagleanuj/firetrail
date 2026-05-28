/**
 * Hand-typed mirror of the small slice of `ft_core::Record` the GUI consumes.
 *
 * ts-rs does not currently export `Record`/`RecordEnvelope`/`Claim` (they live
 * in `ft-core` and carry generic body variants). We deliberately keep this
 * minimal — only the fields the kanban + drawer read.
 *
 * The backend serializes status as snake_case, priority as lowercase, and
 * `RecordId` as a transparent string. Any drift here is a runtime decode
 * problem, not a compile error — keep this in sync with `ft-core::record`.
 */
import type { Relation } from './relation'

export type RecordStatus =
  | 'open'
  | 'ready'
  | 'in_progress'
  | 'review'
  | 'blocked'
  | 'closed'
  | 'deferred'
  | 'archived'

export type RecordPriority = 'p0' | 'p1' | 'p2' | 'p3' | 'p4'

export type RecordKindWire =
  | 'epic'
  | 'task'
  | 'subtask'
  | 'bug'
  | 'incident'
  | 'finding'
  | 'runbook'
  | 'decision'
  | 'gotcha'
  | 'memory'

export interface IdentityWire {
  id: string
  name: string
}

export interface ClaimWire {
  by: IdentityWire
  acquired_at: string
  expires_at: string | null
}

export interface RecordEnvelopeWire {
  id: string
  kind: RecordKindWire
  title: string
  status: RecordStatus
  priority: RecordPriority
  owner: IdentityWire | null
  created_by: IdentityWire
  created_at: string
  updated_at: string
  closed_at: string | null
  owning_scope: string | null
  affected_scopes: string[]
  applies_to: string[]
  labels: Array<{ key: string; value: string }>
}

/**
 * RecordBody is a serde-tagged union in Rust; serialization uses a single-key
 * outer object like `{ "task": { description, ... } }`. The drawer only reads
 * description (when present); everything else is structural.
 */
export type RecordBodyWire =
  | { epic: { description: string } }
  | { task: { description: string; claim?: ClaimWire | null } }
  | { subtask: { description: string; claim?: ClaimWire | null } }
  | { bug: { description: string; claim?: ClaimWire | null } }
  | Record<string, unknown>

export interface RecordWire {
  envelope: RecordEnvelopeWire
  body: RecordBodyWire
}

export interface ShowOutputWire {
  record: RecordWire
  relations: Relation[]
}

export interface CreatedTicketWire {
  record: RecordWire
}

export interface UpdateOutputWire {
  record: RecordWire
  previous_status: string | null
}

export interface ClaimOutputWire {
  record: RecordWire
}

export interface UnclaimOutputWire {
  record: RecordWire
}

export interface CloseOutputWire {
  record: RecordWire
}

/** Pull the description string out of a RecordBodyWire if it carries one. */
export function recordDescription(record: RecordWire): string {
  const body = record.body as Record<string, { description?: string } | undefined>
  for (const key of ['epic', 'task', 'subtask', 'bug'] as const) {
    const inner = body[key]
    if (inner && typeof inner.description === 'string') return inner.description
  }
  return ''
}

/**
 * Pull the trust + risk-class fields out of a memory body. ft-core stores
 * these inside each memory-kind variant (`incident`, `finding`, …) — the
 * outer envelope does not expose them. Ticket bodies don't carry trust,
 * so we return nulls.
 */
export function recordTrust(record: RecordWire): {
  trust: string | null
  riskClass: string | null
} {
  const body = record.body as Record<string, { trust?: string; risk_class?: string | null } | undefined>
  for (const key of [
    'incident',
    'finding',
    'runbook',
    'decision',
    'gotcha',
    'memory',
  ] as const) {
    const inner = body[key]
    if (inner) {
      return { trust: inner.trust ?? null, riskClass: inner.risk_class ?? null }
    }
  }
  return { trust: null, riskClass: null }
}

/** Active claim, if the body carries one. */
export function recordClaim(record: RecordWire): ClaimWire | null {
  const body = record.body as Record<string, { claim?: ClaimWire | null } | undefined>
  for (const key of ['task', 'subtask', 'bug'] as const) {
    const inner = body[key]
    if (inner && inner.claim) return inner.claim
  }
  return null
}

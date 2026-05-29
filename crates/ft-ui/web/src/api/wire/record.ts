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
 * RecordBody is an internally-tagged serde union in Rust
 * (`#[serde(tag = "kind")]`), so the wire shape is a flat object with a
 * discriminator: `{ "kind": "task", "description": "...", "claim": ... }`.
 * Fields live directly on `body`, not under an outer key.
 */
export type RecordBodyWire = {
  kind: RecordKindWire
  description?: string
  summary?: string
  details?: string
  context?: string
  decision?: string
  consequences?: string | null
  body?: string
  root_cause?: string | null
  trust?: string
  risk_class?: string | null
  claim?: ClaimWire | null
} & Record<string, unknown>

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
  const body = record.body
  if (
    (body.kind === 'epic' ||
      body.kind === 'task' ||
      body.kind === 'subtask' ||
      body.kind === 'bug') &&
    typeof body.description === 'string'
  ) {
    return body.description
  }
  return ''
}

/**
 * Pull the trust + risk-class fields out of a memory body. ft-core stores
 * these on each memory-kind variant (`incident`, `finding`, …). Ticket
 * bodies don't carry trust, so we return nulls.
 */
export function recordTrust(record: RecordWire): {
  trust: string | null
  riskClass: string | null
} {
  const body = record.body
  switch (body.kind) {
    case 'incident':
    case 'finding':
    case 'runbook':
    case 'decision':
    case 'gotcha':
    case 'memory':
      return { trust: body.trust ?? null, riskClass: body.risk_class ?? null }
    default:
      return { trust: null, riskClass: null }
  }
}

/** Active claim, if the body carries one. */
export function recordClaim(record: RecordWire): ClaimWire | null {
  const body = record.body
  if (body.kind === 'task' || body.kind === 'subtask' || body.kind === 'bug') {
    return body.claim ?? null
  }
  return null
}

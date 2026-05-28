/**
 * Trust state-machine transitions. Each is a POST against /api/trust/:id/<op>.
 * The backend returns `{ record }` for single-record ops and `{ lastRecord, count }`
 * for merge. We let the type system describe the variants narrowly.
 */
import { apiFetch } from '@/api/client'
import type { RecordWire } from '@/api/wire/record'
import type { EvidenceKindInput } from '@/api/types/EvidenceKindInput'

export interface RecordResponse {
  record: RecordWire
}

export interface MergeResponse {
  lastRecord: RecordWire
  count: number
}

const base = (id: string) => `/api/trust/${encodeURIComponent(id)}`

export function postReview(id: string, reason?: string, evidenceUrl?: string) {
  return apiFetch<RecordResponse>(`${base(id)}/review`, {
    method: 'POST',
    body: { reason: reason ?? null, evidenceUrl: evidenceUrl ?? null },
  })
}

export interface PromoteBody {
  reason?: string
  evidenceUrl?: string
  evidenceType?: EvidenceKindInput
}

export function postPromote(id: string, body: PromoteBody = {}) {
  return apiFetch<RecordResponse>(`${base(id)}/promote`, {
    method: 'POST',
    body: {
      reason: body.reason ?? null,
      evidenceUrl: body.evidenceUrl ?? null,
      evidenceType: body.evidenceType ?? null,
    },
  })
}

export function postDeprecate(id: string, reason: string) {
  return apiFetch<RecordResponse>(`${base(id)}/deprecate`, {
    method: 'POST',
    body: { reason },
  })
}

export function postArchive(id: string, reason?: string) {
  return apiFetch<RecordResponse>(`${base(id)}/archive`, {
    method: 'POST',
    body: { reason: reason ?? null },
  })
}

export function postSupersede(id: string, successor: string, reason?: string) {
  return apiFetch<RecordResponse>(`${base(id)}/supersede`, {
    method: 'POST',
    body: { successor, reason: reason ?? null },
  })
}

export function postRedact(id: string, reason: string) {
  return apiFetch<RecordResponse>(`${base(id)}/redact`, {
    method: 'POST',
    body: { reason },
  })
}

export function postMerge(id: string, sources: string[], reason?: string) {
  return apiFetch<MergeResponse>(`${base(id)}/merge`, {
    method: 'POST',
    body: { sources, reason: reason ?? null },
  })
}

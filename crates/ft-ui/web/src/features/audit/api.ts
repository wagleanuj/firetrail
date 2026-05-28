/**
 * Audit endpoints — lint, verify, review, criteria, diff, graph.
 */
import { apiFetch } from '@/api/client'
import type { LintOutput } from '@/api/types/LintOutput'
import type { VerifyOutput } from '@/api/types/VerifyOutput'
import type { AuditReviewOutput } from '@/api/types/AuditReviewOutput'
import type { CriteriaListOutput } from '@/api/types/CriteriaListOutput'
import type { DiffOutput } from '@/api/types/DiffOutput'
import type { GraphOutput } from '@/api/types/GraphOutput'
import type { GraphDirectionInput } from '@/api/types/GraphDirectionInput'
import type { EvidenceKindInput } from '@/api/types/EvidenceKindInput'

export function postLint(fixHints = true): Promise<LintOutput> {
  return apiFetch<LintOutput>('/api/audit/lint', {
    method: 'POST',
    body: { fixHints },
  })
}

export function postVerify(): Promise<VerifyOutput> {
  return apiFetch<VerifyOutput>('/api/audit/verify', {
    method: 'POST',
    body: {},
  })
}

export function fetchReview(recordId: string): Promise<AuditReviewOutput> {
  return apiFetch<AuditReviewOutput>(
    `/api/audit/review/${encodeURIComponent(recordId)}`,
  )
}

export function fetchCriteria(recordId: string): Promise<CriteriaListOutput> {
  return apiFetch<CriteriaListOutput>(
    `/api/audit/criteria/${encodeURIComponent(recordId)}`,
  )
}

export function addCriterion(recordId: string, text: string) {
  return apiFetch<CriteriaListOutput>(
    `/api/audit/criteria/${encodeURIComponent(recordId)}`,
    { method: 'POST', body: { text } },
  )
}

export function toggleCriterion(recordId: string, which: string, checked: boolean) {
  return apiFetch<CriteriaListOutput>(
    `/api/audit/criteria/${encodeURIComponent(recordId)}/${encodeURIComponent(which)}`,
    { method: 'PATCH', body: { checked } },
  )
}

export interface EvidenceBody {
  url: string
  kind: EvidenceKindInput
}

export function attachCriterionEvidence(
  recordId: string,
  which: string,
  body: EvidenceBody,
) {
  return apiFetch<CriteriaListOutput>(
    `/api/audit/criteria/${encodeURIComponent(recordId)}/${encodeURIComponent(which)}/evidence`,
    { method: 'POST', body: { url: body.url, kind: body.kind } },
  )
}

export interface DiffParams {
  base: string
  head: string
  memoryOnly?: boolean
  scope?: string
}

export function fetchDiff(params: DiffParams): Promise<DiffOutput> {
  const q = new URLSearchParams()
  q.set('base', params.base)
  q.set('head', params.head)
  if (params.memoryOnly) q.set('memoryOnly', 'true')
  if (params.scope) q.set('scope', params.scope)
  return apiFetch<DiffOutput>(`/api/audit/diff?${q.toString()}`)
}

export interface GraphParams {
  id: string
  direction: GraphDirectionInput
  depth: number
}

export function fetchGraph(params: GraphParams): Promise<GraphOutput> {
  const q = new URLSearchParams()
  q.set('id', params.id)
  q.set('direction', params.direction)
  q.set('depth', String(params.depth))
  return apiFetch<GraphOutput>(`/api/audit/graph?${q.toString()}`)
}

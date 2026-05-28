/**
 * Local helper types for the memory surface.
 *
 * The ts-rs `Create*Input` types are too permissive for direct form use
 * (every optional field is typed `T | null`), and the discriminated
 * `POST /api/memory` body is hand-written below to mirror the Rust
 * `CreateMemoryBody` enum in `ft-ui/src/routes/memory.rs`.
 *
 * Keep this in sync with that enum: the JSON wire shape is
 * `{ "kind": "incident" | "finding" | ..., …input fields }`.
 */
import type { CreateDecisionInput } from '@/api/types/CreateDecisionInput'
import type { CreateFindingInput } from '@/api/types/CreateFindingInput'
import type { CreateGotchaInput } from '@/api/types/CreateGotchaInput'
import type { CreateIncidentInput } from '@/api/types/CreateIncidentInput'
import type { CreateMemoryInput } from '@/api/types/CreateMemoryInput'
import type { CreateRunbookInput } from '@/api/types/CreateRunbookInput'

/**
 * Strip `requestId` from a Create*Input: the apiFetch wrapper mints + sends
 * `X-Firetrail-Request-Id` on every write, and the Rust handler copies the
 * header onto the inbound op input — clients don't need to populate it.
 */
type WithoutRid<T> = Omit<T, 'requestId'>

export type MemoryCreateBody =
  | ({ kind: 'incident' } & WithoutRid<CreateIncidentInput>)
  | ({ kind: 'finding' } & WithoutRid<CreateFindingInput>)
  | ({ kind: 'runbook' } & WithoutRid<CreateRunbookInput>)
  | ({ kind: 'decision' } & WithoutRid<CreateDecisionInput>)
  | ({ kind: 'gotcha' } & WithoutRid<CreateGotchaInput>)
  | ({ kind: 'memory' } & WithoutRid<CreateMemoryInput>)

/** Discriminant tag used by the create dialog tabs. */
export type MemoryCreateKind = MemoryCreateBody['kind']

export const MEMORY_KINDS: ReadonlyArray<MemoryCreateKind> = [
  'incident',
  'finding',
  'runbook',
  'decision',
  'gotcha',
  'memory',
] as const

export const RISK_CLASSES = [
  'security',
  'availability',
  'data-loss',
  'compliance',
  'performance',
  'correctness',
] as const

export const SEVERITIES = ['sev1', 'sev2', 'sev3', 'sev4'] as const

export const TRUST_STATES = [
  'draft',
  'reviewed',
  'verified',
  'stale',
  'deprecated',
  'archived',
  'superseded',
  'rejected',
  'redacted',
] as const

export const SEARCH_MODES = ['auto', 'lexical', 'vector', 'hybrid'] as const

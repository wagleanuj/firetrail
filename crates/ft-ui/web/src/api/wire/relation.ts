import type { TicketRelationKind } from '@/api/types/TicketRelationKind'

/**
 * Hand-typed mirror of `ft_core::Relation`. See note in `./record.ts` — ts-rs
 * does not export `Relation`. Keep in sync with `ft-core/src/relation.rs`.
 */
export interface Relation {
  from: string
  to: string
  kind: TicketRelationKind
  created_at: string
  created_by: { id: string; name: string }
}

# Design: Index scopes, identities & audit in ft-search

**Issue:** `firetrail-8z0m.3` (epic `firetrail-8z0m`: Search — make semantic + unified search actually work)
**Date:** 2026-05-29
**Status:** Approved design, pending implementation plan

## Problem

The `ft-search` engine is cross-domain by design but only indexes records that
are `ft_core::Record`s — the 10 `RecordKind` variants (Epic, Task, Subtask, Bug,
Incident, Finding, Runbook, Decision, Gotcha, Memory). Three domains are not
searchable at all:

- **Scopes** — live in `.firetrail/scopes.yaml` (`ft_scope::ScopeRegistry`).
- **Identities** — live in `.firetrail/identities.yaml` (`ft_identity::IdentityRegistry`).
- **Audit/history** — the `history[]` entry chain embedded inside each `Record`
  (ADR-0017); not a standalone store.

> **"Rules" is not a fourth domain.** The epic enumerated "rules, scopes,
> identities, audit", but no first-class Rule entity exists in the codebase or
> docs. Every "rules" reference is generic prose (trust rules, validation rules,
> PR rules) or CODEOWNERS ownership rules — and CODEOWNERS rules live *inside*
> scope definitions. The GUI design doc's Wave 3 confirms the real grouping:
> "scope, identity, audit". So "rules" is folded into scope indexing: a scope's
> CODEOWNERS owners become part of its searchable body text.

### Why this is not "add four match arms"

Two hard architectural assumptions block naive extension:

1. **`lookup_meta` hard-joins the relational `records` table.** Every search hit
   is decorated via `SELECT ... FROM records r ... WHERE r.id = ?1`
   (`engine.rs:436`). An FTS hit with no matching `records` row is silently
   dropped. Synthetic domains have no `records` row.

2. **`RecordId` and `RecordKind` are rigid.** `RecordId::from_string` requires a
   known kind prefix + **exactly 64 lowercase hex chars** (ADR-0015), so
   `apps/checkout` or `alice@example.com` can never be a `RecordId`. And
   `SearchHit.id: RecordId` / `SearchHit.kind: RecordKind` / `SearchQuery.kind_filter:
   Vec<RecordKind>` are all typed to the record model. `search()` calls
   `RecordId::from_string(id_str)` and errors on failure.

## Approach

Model the three domains as **synthetic documents** — searchable text + metadata
with no backing `ft_core::Record`. Decouple search indexing from the relational
`records` table. (Rejected alternatives: promoting them to real `Record`s —
huge ripple through ft-core/ft-index, conflates yaml config with tracked
records, audit has no standalone identity; and a separate per-domain index —
duplicates ranking/query logic and fights the unified-search goal in `.4`.)

## Design

### 1. New ft-search types (`hit.rs`, new `kind.rs`)

```rust
/// Search-layer kind: the record kinds plus the synthetic domains.
pub enum IndexKind {
    Record(RecordKind),   // the 10 existing kinds
    Scope,
    Identity,
    Audit,
}

/// Search-layer document id. Records use their RecordId; synthetic docs
/// use a namespaced key.
pub enum DocId {
    Record(RecordId),
    Synthetic { kind: IndexKind, key: String },
}
```

Synthetic keys:
- Scope → scope id, e.g. `apps/checkout`
- Identity → canonical id/email, e.g. `alice@example.com`
- Audit → `<record-id>#h<n>`, e.g. `TASK-<64hex>#h3`

`DocId` has a stable string form used as the FTS/vec primary key (the `id` /
`id_str` columns are already `TEXT`). It round-trips via `DocId::as_storage_str()`
and `DocId::parse(&str)`:

- **Record** → the bare canonical `RecordId` string (`TASK-<64hex>`). Parsing a
  string that is a valid `RecordId` yields `DocId::Record`.
- **Synthetic** → a domain-tagged form `<tag>:<key>` where `tag` ∈
  `{scope, identity, audit}`: `scope:apps/checkout`,
  `identity:alice@example.com`, `audit:TASK-<64hex>#h3`. The tag makes `parse`
  unambiguous (a Scope vs Identity key cannot otherwise be distinguished). The
  `RecordId` form is tag-free, so the two namespaces never collide (a `RecordId`
  contains no `:`; the `audit:` key embeds a full `RecordId` after the tag).

`SearchHit` changes:

```rust
pub struct SearchHit {
    pub id: DocId,         // was RecordId
    pub kind: IndexKind,   // was RecordKind
    pub title: String,
    pub score: f32,
    pub trust: TrustState,
    pub mode: HitMode,
}
```

`SearchQuery.kind_filter: Vec<IndexKind>` (was `Vec<RecordKind>`).

### 2. Self-sufficient metadata (`schema.rs`, `engine.rs`)

Extend `records_search_meta` so synthetic docs resolve without a `records` row:

```sql
CREATE TABLE IF NOT EXISTS records_search_meta (
    id           TEXT PRIMARY KEY,
    trust        TEXT NOT NULL,
    kind         TEXT,            -- NEW: IndexKind string form
    title        TEXT,            -- NEW
    updated_at   TEXT,            -- NEW (rfc3339)
    owning_scope TEXT             -- NEW
);
```

`lookup_meta` becomes a `LEFT JOIN records r`: when a real `records` row exists,
prefer its `kind/title/updated_at/owning_scope` (records stay authoritative and
unchanged); otherwise fall back to the side-table columns. Trust continues to be
read from `records_search_meta.trust` as today, then kind-default fallback.

Schema migration: `ensure_fts` adds the new columns idempotently (`ALTER TABLE
... ADD COLUMN` guarded by a `PRAGMA table_info` check, or a bumped
`schema_meta` version). Existing rows get the new columns populated on next
rebuild.

### 3. Generalized indexing entry points (`engine.rs`)

```rust
pub struct IndexDoc {
    pub id: DocId,
    pub kind: IndexKind,
    pub title: String,
    pub body: String,
    pub trust: TrustState,
    pub owning_scope: Option<String>,
    pub updated_at: DateTime<Utc>,
}

pub fn upsert_document(&self, doc: &IndexDoc) -> Result<(), SearchError>;
```

`upsert_document` writes the FTS row (DELETE + INSERT, keyed on
`id.as_storage_str()`) and the full `records_search_meta` row (trust + kind +
title + updated_at + owning_scope). The existing `upsert_lexical(&Record)`
becomes a thin wrapper that builds an `IndexDoc` from a record (preserving
today's `record_to_text` and `trust_for_record` behavior) and calls
`upsert_document`.

Widen the vector path for synthetic docs:
- `upsert_vector(&RecordId, …)` → `upsert_vector(&DocId, …)`. The `records_vec`
  table stores `id_str TEXT`, so this is a signature change only.
- `delete(&RecordId)` → `delete(&DocId)`.

### 4. Per-domain extraction (new `crates/ft-search/src/sources.rs`)

Pure functions converting each domain object into an `IndexDoc`. Kept in
ft-search so the engine owns the title/body shape; the reindex command supplies
the domain registries.

| Domain | title | body | trust | owning_scope |
|--------|-------|------|-------|--------------|
| Scope | scope `name` | id + aliases + `applies_to` patterns + CODEOWNERS owner logins | `Verified` (config is authoritative) | self id |
| Identity | display id | canonical id + email + aliases + capability names | `Verified` | none |
| Audit entry | `<op>: <record title>` | author + timestamp + op summaries + trust transition (`draft -> reviewed`) | **inherits parent record's trust** | record's owning_scope |

Trust rationale: scopes/identities are authoritative configuration that should
always survive a `min_trust` filter, so `Verified`. Audit entries inherit the
record's trust so a stale record's history doesn't outrank live content.

### 5. Reindex wiring (`ft-cli/commands/index_cmd.rs`, `rebuild` + `refresh`)

After the existing record loop, in the same pass:

1. `ScopeRegistry::load(&ws.root)` → for each scope, `upsert_document(scope_doc)`.
2. `ft_identity::load_registry(&ws.root)` → for each identity,
   `upsert_document(identity_doc)`.
3. For each record already read from storage, for each `history[]` entry,
   `upsert_document(audit_doc)`.

**Embedding (vector) for synthetic docs happens in this same pass.** For each
synthetic `IndexDoc`, after the lexical upsert, dispatch
`daemon_cmd::ensure_running` + `ft_embed::daemon::send_index_record(socket,
doc.id.as_storage_str(), &text)` where `text` is the doc's title+body. The
daemon embeds and the (widened) `SearchEngineIndexer` writes the vector under
the synthetic `DocId`. Best-effort: if the daemon/model is unavailable the doc
stays lexical-only (consistent with the offline-first / mock-fallback
philosophy). `IndexDoc` carries a canonical `embed_text()` helper so the text
fed to the embedder matches the FTS body.

Deletions are handled by full rebuild's delete-all-then-reinsert; refresh
re-upserts every current doc (no-op for unchanged content at FTS level).

### 6. Consumer updates (the `DocId`/`IndexKind` ripple)

The `SearchHit` type change reaches three isolated conversion points plus the
embedder hook:

- `ft-cli/src/commands/daemon_cmd.rs` — `SearchEngineIndexer::upsert_vector`
  stops forcing `RecordId::from_string`; parses to `DocId` instead (so synthetic
  ids are accepted).
- `ft-cli/src/commands/search.rs` — `From<SearchHit> for SearchHitView` renders
  `DocId`/`IndexKind`; extend the CLI `--kind` arg enum with `scope|identity|audit`
  and map it through `to_index_kind()`.
- `ft-ops/src/memory/search.rs` — `From<SearchHit> for SearchHitOut` and the
  `kind_filter` mapping updated the same way.
- `ft-ui/src/routes/memory.rs` — surfaces the new id/kind shape in JSON; full
  cross-domain UI is `.4`.

### 7. Testing (`ft-search/tests/`, `ft-cli/tests/`)

- ft-search integration: build a fixture workspace with one scope, one
  identity, and a record with ≥2 history entries; assert `upsert_document` +
  `search` returns one hit per domain.
- `kind_filter` isolation: `--kind scope` returns only scope docs, etc.
- Audit per-entry: a record with N history entries yields N searchable audit
  docs with distinct `#h<n>` keys.
- Rebuild backfill: `index rebuild` over an existing fixture corpus populates
  all three domains (lexical assertions; vector assertions gated on the
  `sqlite-vec` feature with a mock embedder).
- CLI acceptance: `firetrail search 'X' --kind scope|identity|audit` returns the
  expected synthetic docs.

## Acceptance criteria

- `firetrail search 'X' --kind scope|identity|audit` returns matching records.
- `firetrail index rebuild` backfills scopes, identities, and audit entries for
  an existing workspace (lexical + best-effort vector).
- Tests cover at least one search hit per newly-indexed domain.

## Explicitly out of scope

- **Incremental on-write embedding for synthetic docs** — re-embedding a scope
  immediately after `scope edit` / `identity update` rather than at next
  rebuild. Follow-up issue.
- **Record-rebuild embedding** — records are still embedded only on-write /
  `migrate`, not at rebuild. Pre-existing gap; a follow-up issue tracks it.
- **Web UI global search** — `firetrail-8z0m.4`.
- **Promoting scopes/identities/audit to real `Record`s.**

## Follow-up issues (filed)

1. `firetrail-8z0m.5` — Incremental on-write embedding for synthetic docs (scope
   edit, identity update, audit-on-record-save).
2. `firetrail-8z0m.6` — Record-rebuild embedding gap (records get vectors only
   on-write/migrate).

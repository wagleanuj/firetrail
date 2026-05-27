# ADR-0002: JSON files in Git as the storage substrate, not Dolt

## Status

Accepted — 2026-05-26

## Context

The original spec proposed Dolt as Firetrail's storage layer. Dolt is a versioned, mergeable SQL database — "Git for SQL tables." It offers row-level three-way merging, which is appealing for a system that wants to track work and memory across parallel branches.

Three problems surfaced when we walked the design end-to-end:

1. **Markdown bodies cannot be cell-merged.** Dolt treats `TEXT` and `BLOB` columns as atomic. Two engineers editing the same incident body on different branches produce an unrecoverable cell conflict. Beads, the closest Dolt-backed prior art, already hits this in production on multi-machine setups.

2. **Engineers already know Git merges.** Adding a Dolt-shaped conflict surface is an adoption tax. Every team member learns a new conflict-resolution tool (FR-126 in the original spec was essentially "build a Dolt merge UX"). That tool's cost is real and rarely justified.

3. **The row-merge magic Dolt sells is mostly idle.** The Firetrail workload is overwhelmingly append-and-evolve: records are created, status changes, evidence is added, links accumulate. Two engineers editing the same field of the same record on different branches is genuinely rare. Git's line-level three-way merge handles the common cases; the rare ones can use a small custom JSON merge driver.

Our stated criteria for the design — **speed to ship** and **workability with teams** — point strongly away from Dolt.

## Decision

Store each record as a single JSON file under `.firetrail/records/<type>/<id>.json`, version-controlled by Git. SQLite plus `sqlite-vec` is a derived read index, rebuildable from the files at any time. The vector embedding cache is content-hash keyed and machine-local.

## Consequences

Positive:

- Git is the storage engine and the merge engine. No sidecar process, no `dolt sql-server` lifecycle, no port or socket management.
- Engineers resolve record conflicts the same way they resolve code conflicts — in their editor, with `git mergetool`, with familiar diffs.
- PR review of memory works natively. Reviewers read JSON diffs in the same UI they already use for code.
- A custom Git merge driver for `.firetrail/records/**/*.json` handles structured edge cases (array order, criteria lists) deterministically, without forcing humans to learn it.
- The read index is throwaway. Index corruption is a rebuild, not a recovery exercise.
- One coherent mental model — files in Git. Onboarding cost drops.

Negative:

- No row-level merge magic for the rare case of concurrent edits to the same field of the same record. Mitigated by the custom merge driver and by the natural rarity of this pattern.
- A query that would have been a single SQL statement against Dolt is now a SQLite query against the rebuilt index. Acceptable — index is fast and rebuilds incrementally.
- The history-bloat problem (every mutation appending to the JSON) needs explicit handling. Addressed by ADR-0003.

## Alternatives considered

**Dolt embedded via the Go driver.** Sidesteps the sidecar process but inherits all three problems above. The blob-merge limitation alone is disqualifying for an incident-memory system whose value depends on long-form prose surviving merges.

**Dolt as a sidecar `sql-server`.** Adds process lifecycle complexity for no advantage over the embedded path.

**SQLite as the primary store, committed to Git as a binary file.** SQLite database files do not merge. Two branches that both edit the database produce an irresolvable binary conflict. Disqualified.

**Sanakirja (Pijul's MVCC key-value store).** Genuine branch primitives, but no SQL or table layer — we would build merge logic ourselves and rediscover Dolt's design space without the maturity. Not justified at v1.

**Automerge (CRDT documents).** Mature and embeddable, but CRDT semantics replace three-way merge with automatic convergence. You cannot reject a merge or surface a conflict — wrong fit for an audit-grade memory system.

**Custom prolly-tree implementation.** Reinvents Dolt's storage engine. Disqualified on cost.

## References

- ADR-0003: PR-time history compaction (addresses growth concern)
- ADR-0006: Storage modes (embedded vs. external)
- Backlog.md — production proof that JSON-files-in-Git is workable at scale

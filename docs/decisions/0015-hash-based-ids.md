# ADR-0015: Hash-based record IDs with full hash storage and short display

## Status

Accepted — 2026-05-26

## Context

Record IDs must be allocated without coordination. Two engineers on two branches creating records simultaneously must produce different IDs, and the resulting branches must merge cleanly without ID collisions.

Sequential IDs (`TASK-882`, `TASK-883`) work only with a central counter, which Firetrail explicitly does not want. Hash-based IDs solve the coordination problem but raise their own questions:

1. **Collision probability.** A 6-hex-character ID (24 bits, 16M space) hits birthday-paradox collisions around ~4,000 records. At team scale across years, that is not safe.
2. **Display ergonomics.** Full SHA-256 IDs are unreadable. `TASK-7f2a91...` is bearable; `TASK-7f2a915c3d4e5b6a8c0d1e2f...` is not.
3. **Reference robustness.** A reference written as `TASK-7f2a91` becomes ambiguous if a future record's ID also starts with `7f2a91`.

## Decision

### Full content hash stored, short prefix displayed

Each record's filename and `id` field carry the full content-derived hash:

```
.firetrail/records/task/TASK-7f2a915c3d4e5b6a8c0d1e2f3a4b5c6d.json
```

The CLI displays a short prefix by default, expanded to whatever length is unambiguous in the current view:

```
$ firetrail task list
TASK-7f2a91  Add Redis pool alert       in-progress
TASK-9c4b2e  Refactor cache layer        ready
TASK-9c4b3a  Investigate retry storm     ready

# Default display: 6 hex chars (sufficient for this view)
# CLI auto-extends if a prefix is ambiguous:
TASK-9c4b2e  Refactor cache layer        ready
TASK-9c4b3a  Investigate retry storm     ready
```

References (in commit messages, PR descriptions, other record fields) accept any prefix length that resolves uniquely in the repository's current record set. The CLI normalizes references to the full ID at write time.

### Hash derivation

The hash is computed at record-create time from:

- A random 128-bit nonce (ensures distinct simultaneous creations differ).
- The creating identity (canonical form).
- The record `type`.
- The current timestamp at millisecond precision.

SHA-256 of the concatenation, taken as the full ID. The nonce is the primary collision-avoidance device; the other inputs ensure deterministic distinctness when the nonce machine state is poorly seeded.

The hash is *not* derived from the record body. Bodies change over the record's lifetime; the ID does not.

### Collision handling

A collision on the full hash is cryptographically impossible at any realistic team scale. The system does not check for collisions on the full ID.

A collision on the displayed prefix is detected and resolved at display time:

- Display logic walks the current record set, finds the shortest unique prefix for the ID being shown, and uses it.
- A minimum prefix length (default 6) keeps short IDs human-readable.
- A maximum-detected ambiguity exceeding a threshold (e.g. >50% of records share a 6-char prefix) is a `firetrail doctor` warning.

### ID rekeying

In the rare case that two engineers create records with the same intent and the merge driver detects a logical duplicate (different IDs, same content within a similarity threshold), `firetrail id rekey <id>` reassigns one record to a new hash and updates inbound references. This is a tool for the duplicate-resolution workflow, not for general use.

### Storage path

IDs are case-sensitive and lowercase. File paths use lowercase IDs to remain consistent across case-sensitive and case-insensitive filesystems. `firetrail doctor` rejects records with mixed-case IDs.

## Consequences

Positive:

- No central counter, no coordination, no race conditions at create time.
- Cryptographically safe at full-ID level.
- Human-readable display via adaptive prefix.
- References in older PR descriptions or commit messages stay valid even as the record set grows, because resolution is prefix-based.

Negative:

- IDs do not encode creation order. Sorting by ID is meaningless. Sorting must use `created_at`. Acceptable.
- Filename lengths are long. Acceptable on modern filesystems.
- Prefix ambiguity at display time requires recomputation when the set changes. Acceptable cost — happens once per command, in memory.

## Consequences for design

- The record schema requires `id` to be the full hash.
- The CLI's display layer maintains an in-memory prefix-disambiguation map per command invocation.
- The merge driver compares records by full ID, not by prefix.
- The `firetrail history`, `firetrail show`, and similar commands accept any unambiguous prefix and expand it.
- Audit log entries record full IDs.

## Alternatives considered

**Sequential IDs with a central allocator.** Disqualified by the no-coordination requirement.

**UUIDv4 IDs.** Equivalent collision safety, less readable, no advantage over content-derived hash with prefix display.

**UUIDv7 (time-ordered UUIDs).** Provides creation-order via the ID, but the resulting ID still requires prefix-display logic for human readability. Marginal benefit; the time-order property is also obtainable via `created_at`. Considered as an alternative encoding; final choice remains content-hash for simplicity.

**6-character hash, accept collision risk.** Disqualified by the birthday-bound at ~4,000 records.

**Type-scoped numbering (TASK-1, TASK-2 within a single record type).** Better readability but reintroduces coordination across branches. Disqualified.

## References

- ADR-0008: Identity registry (the canonical identity used in hash derivation)
- ADR-0002: JSON-in-Git storage (the path layout that uses full IDs)

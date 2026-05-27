# ADR-0017: Audit chain integrity — prev_state_hash, verify, and force-push protection

## Status

Accepted — 2026-05-26

## Context

The audit log of a record's lifetime is part of Firetrail's value. Who promoted this finding to verified? When did the runbook last change? Did anyone tamper with the trust state?

The original spec assumed `git log` would serve as the audit log. Two failure modes break that assumption:

1. **Git history is rewritable.** `git rebase`, `git filter-branch`, `git commit --amend`, and `git push --force` all produce a different history than the one previously seen. An adversary or a careless engineer can rewrite the author or content of past commits.
2. **Squash-merge collapses authorship.** Already addressed by ADR-0003 (in-record history). But the cross-PR chain itself can still be rewritten if force-pushed.

Without tamper-evidence, the trust model (ADR-0013) is a polite request. Anyone who can write to the repo can also rewrite the audit trail to make any record appear to have any history.

## Decision

### `prev_state_hash` chain

Each record carries:

- `state_hash` — SHA-256 of the record's current canonical-form serialization (excluding `state_hash` and `prev_state_hash` themselves).
- `prev_state_hash` — the `state_hash` of the previous version of this record at the last merge into `main` (or equivalent long-lived branch).

When a PR-merge compaction (ADR-0003) writes a new `history[]` entry, it also updates `state_hash` and sets `prev_state_hash` to the prior `state_hash`. The two together form a Merkle chain across the record's main-branch history.

If a force-push rewrites a commit that touched the record on `main`, the `prev_state_hash` chain breaks: the current record's `prev_state_hash` no longer matches any actual prior state's `state_hash`.

### `firetrail verify` command

```
firetrail verify [<record-id>] [--all] [--scope <s>]
```

Walks the record's history chain and validates:

- Each entry's claimed `from_hash` matches the prior entry's `to_hash`.
- The current `state_hash` matches the actual content hash of the record.
- The `prev_state_hash` resolves to a real ancestor in `git log --follow` on the record file.

Reports breaks at the field and history-entry level. Optional `--repair` flag rebuilds the chain from the current state forward, requiring an explicit reason recorded in the audit log.

### Force-push protection

A server-side Git pre-receive hook (shipped as a configurable artifact) rejects force-pushes that touch `.firetrail/records/**`. Teams that enable the hook get hard tamper-evidence at the Git transport layer.

For teams that cannot install server-side hooks (limited GitHub permissions on a fork, for example), the same check runs in `firetrail check pr`: a PR whose target branch's `.firetrail/records/` tree has been force-overwritten relative to the PR's expected ancestor fails the check.

### Signed commits (optional)

Teams that want cryptographic identity binding can configure Firetrail to require GPG- or sigstore-signed commits on paths under `.firetrail/records/`. The CLI accepts the configuration and refuses to write unsigned commits when the policy is set.

This is opt-in. Most teams operate under the simpler model where `firetrail verify` plus the force-push hook are sufficient.

### Integrity at the embedding cache layer

A separate concern, sharing the integrity theme. Each embedding cache row stores `(content_hash, model_id, model_version, vector, vector_checksum)`. `firetrail doctor` samples N rows and re-embeds them to detect silent corruption. The daemon refuses to mix vectors from different `model_id` or `model_version` values, so model upgrades are explicit migrations rather than silent skew.

### Integrity at the read index layer

The SQLite read index is rebuildable from the record files. `firetrail doctor` performs a delta check between the index and the current Git tree's record files and reports drift. `post-checkout` and `post-merge` git hooks installed by `firetrail init` reconcile the index automatically after branch movement.

## Consequences

Positive:

- Audit history is tamper-evident at the record level. Force-pushes that rewrite history are detected.
- `firetrail verify` provides a clear test of integrity for any record or the whole corpus.
- The integrity guarantees compose with `git log` rather than replacing it — Git remains the primary audit transport, the in-record chain is the cross-check.
- Teams that want stronger guarantees can layer on server-side hooks and signed commits without changing the data model.
- Embedding-cache corruption is detectable rather than served as confident wrong answers.

Negative:

- A small amount of overhead per write: compute the new `state_hash`, set `prev_state_hash`. Negligible.
- `firetrail verify --all` on a large corpus is not instant. Acceptable — runs in CI nightly and on demand, not on every command.
- Repair operations require human judgment. `--repair` writes audit-log entries explaining what was rebuilt and why.

## Alternatives considered

**Git history alone, no in-record chain.** Already shown insufficient: force-push and squash both break it.

**External audit log file.** Adds a parallel file to merge. Inherits its own conflict surface. No advantage over the in-record chain.

**Append-only WORM store outside Git.** Strong tamper-evidence but breaks the JSON-in-Git architecture. Disqualified — the storage substrate (ADR-0002) is the source of truth.

**Hash chain across all records (one global chain).** Strong global integrity but causes write contention and merge complexity. Per-record chains are sufficient.

## References

- ADR-0002: JSON-in-Git storage
- ADR-0003: PR-time history compaction
- ADR-0007: Embedding cache integrity
- ADR-0013: Trust model (the workflows the chain protects)

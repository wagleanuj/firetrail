# ADR-0003: Record history is compacted at PR merge, not stored per mutation

## Status

Accepted — 2026-05-26

## Context

A record file must encode provenance that survives Git operations which rewrite history (squash merge, rebase, force-push). The naive solution is an append-only `history[]` array inside the record JSON, with one entry per mutation. This works correctness-wise but bloats records linearly with activity: a record touched in 12 commits across one PR carries 12 history entries forever, even though main only saw one merge.

We need bounded growth without losing the provenance that the trust model depends on.

## Decision

Each record carries:

- The full current state of all fields.
- A `state_hash` — content hash of the canonical record state.
- A `prev_state_hash` — content hash of the state this record had at the previous merge into the long-lived branch.
- A `history[]` array containing one entry per *merged PR* that touched the record. Not per mutation.

During PR development on a feature branch, mutations append to a transient `_pending` array. At PR merge, a compaction step (run as a PR check) collapses `_pending` into a single `history[]` entry summarizing the PR's effect — primary actor, contributors, ops summary, ops count, from-hash, to-hash, PR number — and clears `_pending`.

A record's history grows linearly with merged PRs that touched it, not with the underlying mutation count.

Secondary compaction is available for records that accumulate many PRs over years: entries older than a configurable window (default one year) are collapsed into yearly buckets that preserve contributors, state transitions, and PR counts.

## Consequences

Positive:

- Bounded JSON size. A record touched by 30 PRs over five years carries roughly 30 history entries (~6 KB), not 300+.
- Audit granularity matches the granularity of shipped changes. Main only ever saw PR-level changes; main's history reflects that.
- The `prev_state_hash` chain is tamper-evident across the long-lived branch. Force-pushes that rewrite record content are detected by `firetrail verify`.
- Squash-merge safe. The PR's individual commits are collapsed by Git on squash; compaction had already collapsed them in-record before the squash ran.
- Rebase-safe. `state_hash` is content-derived. Rebasing preserves the chain.
- Engineers retain fine-grained development-time history in `_pending` while working on the branch. They lose it on merge, but the feature branch's Git commits still exist and the PR description preserves the long-form trace.
- A record's full PR-grained history can be reconstructed by walking `history[]`. Cross-references via `merged_via_pr` resolve to the original PRs in GitHub or the data repo.

Negative:

- Compaction is a step that can fail or be skipped if the PR check is bypassed. Mitigated by making the compaction check part of `firetrail check pr` and running it on every PR touching `.firetrail/records/`.
- Within-PR mutation provenance is lost on merge. Acceptable — main only cares about what shipped, not every keystroke. The PR description and the feature branch's Git commits remain available for archaeology.
- Secondary compaction is lossy. Five-year-old PR-grained history collapses to a yearly summary. Acceptable for the same reason.

## Consequences for tooling

- `firetrail compact --pr <n>` runs the compaction step idempotently.
- Pre-merge CI check rejects records with a non-empty `_pending` array.
- `firetrail history <record-id>` reconstructs and prints the timeline.
- `firetrail verify <record-id>` validates the `prev_state_hash` chain end-to-end.
- The Git merge driver for record JSON treats `history[]` as append-with-dedup-by-PR-number to handle rare cases where two PRs touching the same record interleave.

## Alternatives considered

**Per-mutation `history[]`.** Bloats linearly. Disqualifying at scale.

**No in-record history; rely entirely on `git log`.** Squash-merge collapses authorship onto the merger. Force-push rewrites history silently. The `git log` approach fails the audit requirement.

**External history store (`.firetrail/history/<id>.jsonl`).** Adds a parallel file system to merge. Inherits its own conflict surface. No advantage over in-record `history[]`.

**Git notes.** Survive squash if explicitly pushed but are not pushed by default and have poor tooling UX. Notes are also rewriteable, so they do not solve the tamper-evidence requirement.

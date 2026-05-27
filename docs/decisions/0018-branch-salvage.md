# ADR-0018: Branch salvage is opt-out, not opt-in

## Status

Accepted — 2026-05-26

## Context

In the JSON-in-Git storage model (ADR-0002), records live on branches. When a feature branch is abandoned or deleted, the records on that branch are lost unless salvaged.

The original design had an opt-in salvage command:

```
firetrail memory salvage
```

The engineer was expected to remember to run it before deleting their branch. In practice, this is a "forget-in" — engineers will not remember, and the loss is silent. Stress-testing flagged this as a critical failure mode (S8 in the Git-lifecycle scenarios).

For a system whose value depends on preserving team learning, the default cannot be "lose it unless someone remembers to save it."

## Decision

Branch salvage is opt-out. When Firetrail detects a branch about to be deleted (or already deleted), it surfaces a salvage prompt for records on that branch that are not also on `main`. The default action depends on record type.

### Detection

Salvage detection runs on three triggers:

1. **Local `post-checkout` hook installed by `firetrail init`.** Detects branches that disappear from the local repo (e.g., after `git branch -D`). Prompts the engineer.
2. **GitHub webhook** (or equivalent) on `delete` events. Triggers a salvage notification in the CI environment.
3. **Explicit command.** `firetrail memory salvage --branch <name>` runs the workflow on demand.

### Default actions per record type

```
finding   → salvage by default (Y)
decision  → salvage by default (Y)
runbook   → salvage by default (Y)
incident  → salvage by default (Y)
gotcha    → salvage by default (Y)
memory    → salvage by default (Y)
task      → do not salvage by default (N)
subtask   → do not salvage by default (N)
bug       → do not salvage by default (N)
```

The reasoning: knowledge records (findings, decisions, incidents, runbooks, gotchas, memory) represent team learning and should survive. Workflow records (tasks, subtasks, bugs) are scoped to the branch's intended change — if the branch is abandoned, the task is implicitly abandoned too.

### Salvage workflow

When triggered:

```
Branch `feature-redis-alerts` was deleted. 4 records exist on this branch
that are not on `main`:

  [Y] FIND-9c4b2e   Redis pool exhaustion appears before CPU alarms
  [Y] DEC-44a1c3    Bounded retry policy for checkout
  [N] TASK-7f2a91   Add Redis pool saturation alert
  [Y] RUN-311abc    Inspect Redis pool saturation

Salvage selected records via a memory-only PR to main? [Y/n]
> Y

Creating branch `firetrail/salvage-feature-redis-alerts`
  ✓ Cherry-picked FIND-9c4b2e
  ✓ Cherry-picked DEC-44a1c3
  ✓ Cherry-picked RUN-311abc
Pushed branch. Open PR: https://github.com/org/repo/pull/892
```

The engineer can override per record by toggling the [Y]/[N] flags in an interactive prompt.

### Headless mode

In CI or non-interactive contexts (the webhook trigger after a remote branch delete), Firetrail cannot prompt. Instead it opens a memory-only PR with the default selections and assigns it to the branch's original author for review. The author receives a notification and can adjust the PR before merging.

### Discarding without salvage

`firetrail memory salvage --branch <name> --discard` explicitly skips salvage. Writes an audit-log entry naming the operator and the records discarded.

### Idempotency

Running salvage on a branch that has already been salvaged is a no-op with a notice. The CLI tracks which branches have been processed via local state.

## Consequences

Positive:

- Team learning survives branch abandonment by default. Engineers do not have to remember a separate step.
- Knowledge records (findings, decisions, runbooks, incidents) follow the durability promise even when their motivating code change does not ship.
- Workflow records (tasks) are correctly let go when their branch dies — they have no value outside the change they represented.
- CI-triggered salvage covers the case where engineers delete branches via GitHub UI without running any local command.

Negative:

- Adds a PR per abandoned branch with records. Most abandoned branches do not have records, so the practical PR volume is small.
- Headless salvage creates PRs that may sit open if their assignee does not act. Mitigated by stale-PR detection in `firetrail doctor` and by team conventions around assignment.
- A salvage PR may include records the author did not intend to land. Mitigated by the per-record toggle in interactive mode and by the PR review step in headless mode.

## Consequences for design

- `firetrail init` installs the `post-checkout` and `post-merge` hooks.
- A small GitHub Action (`firetrail-salvage-on-delete`) is shipped as a template for teams to install.
- The local state tracking which branches have been salvaged lives in `~/.cache/firetrail/<repo>/salvage-state.json` (machine-local, not Git-tracked).
- Salvage interacts with the memory-only PR workflow (ADR-0009). The salvage PR follows the same rules.

## Alternatives considered

**Opt-in salvage.** The original design. Disqualified by the silent-loss failure mode.

**Auto-promote all records to main without PR review.** Loses the review step. Disqualified — bypassing review for the sake of durability creates a new vector for poisoning the main corpus.

**Per-team policy on what salvages by default.** Possible later. v1 ships the type-based defaults above.

**Never delete records, even from abandoned branches.** Keeps everything but pollutes main with speculative drafts. Disqualified.

## References

- ADR-0009: Memory-only PRs (the workflow salvage uses)
- ADR-0013: Trust model (salvaged drafts still start as drafts)

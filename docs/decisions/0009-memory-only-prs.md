# ADR-0009: Memory-only PRs for findings, decisions, incidents, runbooks

## Status

Accepted — 2026-05-26

## Context

When a record file is committed in the same Git commit as code changes, the record's fate is tied to the code's fate. Reverting the code merge also reverts the record. This breaks Firetrail's core promise.

Two concrete failures:

1. **Revert of a merge commit.** A bad code fix is reverted on `main`. The finding that documented the underlying problem is reverted with it. The finding was epistemically true — the team just rolled back the wrong implementation. The lesson is now lost.

2. **Long-lived feature branches.** Engineers learn things during multi-week feature work that the rest of the team needs immediately. If memory only lands when the code lands, everyone else operates on stale knowledge for weeks.

The classes of record that need durability beyond any single PR are: `finding`, `decision`, `incident`, `runbook`. These represent production-truth and team-truth. They should outlive the code changes that prompted them.

The classes scoped to a specific change — `task`, acceptance criteria, evidence references — can ship with the code, because they describe the change itself.

## Decision

Records of type `finding`, `decision`, `incident`, and `runbook` must land via memory-only PRs — PRs that touch only `.firetrail/records/` and never code paths. This is enforced by a pre-commit hook installed by `firetrail init` and re-enforced in CI by `firetrail check pr`.

Records of type `task`, `subtask`, `bug`, `acceptance_criterion`, and `evidence` are allowed to co-commit with code changes. They are tied to the change by design.

### Workflow

When an engineer creates a finding mid-feature:

```bash
firetrail finding create "Redis pool exhaustion appears before CPU alarms"
# Created FIND-9c4b2e on current branch (status: draft)

firetrail memory promote-to-main FIND-9c4b2e
# Creates a new branch firetrail/finding-9c4b2e off main
# Cherry-picks the record file onto it
# Pushes the branch and opens a memory-only PR
# Returns the PR URL
```

The engineer's feature branch continues unaffected. The finding's PR is reviewed and merged independently — usually faster than the code PR, because memory PRs are small and tightly scoped.

### Incidents

Incidents are treated even more aggressively. `firetrail incident create` opens a memory-only PR to main on creation, not after the fact. Production reality is shared as soon as it is known, not gated on a fix landing months later.

### Failure mode

If an engineer attempts to commit a `finding`/`decision`/`incident`/`runbook` file alongside code changes, the pre-commit hook blocks the commit with a clear message:

```
error: finding records cannot be committed with code changes.
       Run `firetrail memory split` to move them to a memory-only commit,
       or `firetrail memory promote-to-main FIND-xxx` to open a separate PR.
```

CI runs the same check. PRs that mix memory and code on the relevant record types fail `firetrail check pr`.

## Consequences

Positive:

- Reverting a code PR cannot revert team learning. Findings, decisions, incidents, and runbooks survive.
- Cross-team knowledge flows in days, not weeks. Memory PRs land before their motivating code PRs do.
- Incident records are visible to everyone immediately. On-call learns from yesterday's incident, not from yesterday's eventually-merged fix.
- Review audience is correctly partitioned. Code PRs go to code reviewers; memory PRs go to memory reviewers (typically owning team plus relevant subject-matter experts via CODEOWNERS).
- Memory PR diffs are small and easy to review. Trust earned by review (ADR-0013) is more meaningful when reviewers actually read the change.

Negative:

- More PRs in flight per engineer. Mitigated by memory PRs being small, fast to review, and often auto-mergeable when they meet quality criteria.
- The `promote-to-main` step adds friction. Mitigated by it being one command, and by the Claude Code skill instructing the agent to run it automatically after `firetrail capture`.
- An engineer might create a finding on a branch and forget to promote it. Branch salvage (ADR-0016) catches this on branch deletion.

## Alternatives considered

**Convention only — encourage memory-only PRs but do not enforce.** Conventions decay. The first time an engineer races to ship a fix, the convention will be ignored and the finding will die with the next revert. Rejected.

**Allow all records to co-commit; rely on `firetrail memory restore` to revive reverted records.** Possible but post-hoc — by the time someone realizes the finding is gone, it has already been missed. Rejected.

**Restrict the rule to `incident` only.** Incidents are the most acute case, but findings, decisions, and runbooks have the same durability argument. Rejected as too narrow.

## References

- ADR-0003: PR-time history compaction (memory-only PRs still benefit from compaction)
- ADR-0016: Branch salvage (rescues memory before branch deletion)
- ADR-0013: Trust model (review process for memory PRs)

# ADR-0010: PR-link enforcement for cross-repo references

## Status

Accepted — 2026-05-26

## Context

In external storage mode (ADR-0006), records live in a separate data repo from the code they relate to. A code PR may close `TASK-7f2a91`, reference `INC-abc123`, or rely on `DEC-44`. Those records live in a different repo with a different PR lifecycle.

Without enforcement, three failure modes appear:

1. **Dangling references.** A code PR claims to close `TASK-7f2a91`, but `TASK-7f2a91` does not exist in the data repo. The reference is a guess or a typo and nothing catches it.

2. **Drift.** The code PR lands. The data-repo PR that was supposed to update `TASK-7f2a91`'s status never opens or lingers. Records and reality drift apart.

3. **Audit gaps.** A revert of the code PR leaves the data repo's records claiming the change is in effect. The history of what actually shipped is now wrong.

In embedded mode the analogous concern is much smaller — records and code share commits, so references resolve trivially. But cross-PR references inside one repo (e.g. an open feature PR referencing a record edited in a separate memory-only PR) have the same drift potential.

## Decision

`firetrail check pr` enforces PR-link integrity. The rules differ by storage mode and PR kind.

### Embedded mode

- Code PRs that reference records (`closes: TASK-xxx`, `relates-to: INC-yyy`) must resolve those records to entries that exist somewhere in the repo's record tree — current branch, main, or any open PR's head.
- A code PR that references a record being introduced by a separate memory-only PR must include that memory PR's number in its description: `firetrail-pr: #892`.
- CI walks the references and fails the check on any unresolved reference.

### External mode (default `loose` atomicity)

- Code PR descriptions must include a `firetrail-data-pr:` line whenever the PR references records that do not yet exist on `main` of the data repo.
- The data PR must exist (open, merged, or closed) and must touch the referenced records.
- CI does not block the code PR on the data PR's merge status. Engineers are free to land them in any order.
- A code PR may merge before its data PR; CI emits a warning that resolves once the data PR lands.

### External mode (`strict` atomicity)

- All rules of `loose` mode apply.
- CI blocks the code PR until every referenced record exists on `main` in the data repo with the relevant fields updated.

### Reference syntax

Records are referenced via standardized footers in the PR description or commit messages:

```
firetrail-closes: TASK-7f2a91
firetrail-relates: INC-abc123, FIND-9c4b2e
firetrail-data-pr: org/firetrail-data#421
```

A `firetrail-` prefix avoids collision with other conventions. References are case-insensitive and whitespace-tolerant.

### Enforcement runs

- `firetrail check pr` on every PR open and synchronize.
- `firetrail doctor --refs` runs the same validation across all open PRs on demand.
- Pre-commit hooks may optionally warn on local commits referencing unresolved records.

## Consequences

Positive:

- Dangling references are caught at the moment they enter the system, not months later.
- External mode's atomicity gap is contained. A code PR may merge ahead of its data PR, but the link is mandatory and machine-checked.
- Audit reconstruction is possible. Given any code commit, walking `firetrail-` footers resolves to the records that describe its intent.
- Reverting a code PR carries the references with it. The data repo's records can be updated by a follow-up PR that re-references the revert.

Negative:

- More PR description discipline. Mitigated by `firetrail` CLI inserting the footers automatically when records are referenced via commands (`firetrail close TASK-xxx` writes the `firetrail-closes:` footer into the current commit message or PR draft).
- External-mode `loose` policy still permits drift in the gap between merge and data-PR landing. Acceptable trade for adoption speed; teams that cannot tolerate the gap can choose `strict`.

## Alternatives considered

**No enforcement; rely on PR-description conventions.** Conventions decay under deadline pressure. Rejected.

**Block code PRs on data-PR landing in all cases.** `strict` atomicity by default. Too high friction for v1 adoption — teams should be able to choose. Rejected as default.

**Bidirectional links — data PRs must also reference back to the code PR.** Useful but creates a chicken-and-egg problem at PR open. Made an opt-in feature for the `strict` policy.

## References

- ADR-0006: Storage modes
- ADR-0009: Memory-only PRs (the primary case of cross-PR references)

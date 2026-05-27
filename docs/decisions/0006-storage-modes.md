# ADR-0006: Two storage modes — embedded and external

## Status

Accepted — 2026-05-26

## Context

Engineering teams are organized two common ways:

1. **Single repository.** A monorepo or a small product repo. Code, tasks, incidents, and memory all relate to the same codebase.
2. **Multiple repositories.** Microservice organizations where a single team owns several services across several repos, or several teams contribute to a shared knowledge base while owning separate code repos.

A single storage model does not serve both well. Embedding `.firetrail/` directly in a code repo is the obvious choice for case 1 but creates real friction in case 2: every code repo carries a parallel records directory, knowledge does not flow across repos, and code reviewers see memory PRs interleaved with code PRs.

Conversely, storing records in a separate "data repo" is natural for case 2 but introduces atomicity and discovery costs that small teams in a single repo would not want to pay.

## Decision

Firetrail supports two storage modes, selected at `firetrail init` time. The setup flow explicitly asks the team which fits.

### Embedded mode

Records live in `.firetrail/records/<type>/<id>.json` inside the code repo itself. Tasks, incidents, findings, and code changes are version-controlled together. PR review of memory happens in the same PR as the code that motivated it (or as separate PRs in the same repo — see ADR-0009).

### External mode

Records live in a separate Git repository — the *data repo* — typically named `<org>/<product>-firetrail` or `<org>/firetrail-data`. The code repo contains only a small `.firetrail.toml` config file pointing to the data repo. Firetrail clones the data repo to a local cache (`.firetrail/cache/`, gitignored) and reads and writes against that clone. Memory PRs are opened in the data repo, separately from code PRs.

External mode supports multiple code repos pointing at one data repo. Knowledge accumulated by one team is searchable by every team sharing the data repo, with scope-based partitioning (ADR-0004) preserving team boundaries.

### Both modes share

- The same record schema, CLI commands, indexing, embedding, trust model, and review workflow.
- The same `firetrail check pr` integration in CI.
- The same Git merge driver for record JSON files.

## Consequences

Positive:

- Single-repo teams get atomicity, branch parity, and zero setup cost.
- Multi-repo organizations get shared memory across services and clean code repos.
- Compliance-sensitive teams that need memory and audit data in a separately-permissioned repo can use external mode without changing workflow.
- The storage abstraction stays small. Both modes implement the same trait; differences are confined to setup, the PR-link flow, and `firetrail doctor` checks.

Negative:

- External mode has weaker atomicity than embedded mode. Code and memory PRs are independent — a code PR can land before its referenced memory PR lands in the data repo. Mitigated by mandatory PR-link enforcement: code PRs must reference the data-repo PR number, and CI validates the link resolves. See ADR-0010.
- External mode requires more setup (create data repo, configure auth, agree on naming).
- Discoverability is lower in external mode. A new engineer cloning the code repo does not see Firetrail context until they install the CLI and run `firetrail doctor`, which bootstraps the data repo clone.

## Atomicity policies in external mode

Three policies are supported, selected at init:

- **`loose`** (default). Code PR and data PR are independent. CI validates that record references in the code PR resolve to records that exist somewhere (any branch) in the data repo. Engineers land them in a reasonable order.
- **`strict`**. CI in the code repo blocks the code PR until the referenced records exist on `main` in the data repo.
- **`auto-sync`**. A bot creates matching branches and PRs in the data repo whenever a code branch is created. Merge of the code branch triggers merge of the data branch. Higher infrastructure cost — out of scope for v1.

Firetrail v1 ships `loose` only. `strict` and `auto-sync` are deferred.

## Mode selection at init

```
firetrail init

? How is your team's code organized?
  ▸ Single repository / monorepo
    Multiple repositories (microservices)
    Not sure — explain

> Single repository / monorepo
✓ Mode: embedded
✓ Created .firetrail/
```

```
firetrail init

? How is your team's code organized?
    Single repository / monorepo
  ▸ Multiple repositories (microservices)
    Not sure — explain

> Multiple repositories
? Data repo URL: git@github.com:org/firetrail-data.git
? Atomicity policy: loose / strict
✓ Mode: external
✓ Created .firetrail.toml
✓ Cloned data repo to .firetrail/cache/
```

## Alternatives considered

**Embedded only.** Loses the multi-repo organization use case, which is common at team scale. Rejected.

**External only.** Forces all single-repo teams through unnecessary setup and atomicity-loss. Rejected.

**A third "skill-only / markdown-only" mode** with no CLI installed. Considered and rejected for team scale (ADR-0012). Solo-developer adoption was the only scenario this served and is not the target audience.

## References

- ADR-0009: Memory-only PRs (relates to atomicity)
- ADR-0010: PR-link enforcement (mandatory in external mode)
- ADR-0012: Skill is agent documentation, not a separate tier

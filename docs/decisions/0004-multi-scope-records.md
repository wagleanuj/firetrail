# ADR-0004: Records carry multi-scope routing, not a singleton scope label

## Status

Accepted — 2026-05-26

## Context

In a monorepo, records (tasks, incidents, findings, decisions) frequently span more than one team or service. Real examples:

- A checkout-team incident is triggered by a payment-team change.
- A finding about a shared Redis cluster applies to every service that uses it.
- A decision in shared infrastructure code affects every consumer.

A singleton `scope` label routes the record to one team and silences it for everyone else. Stress-testing the design exposed three concrete failures from this:

1. Cross-scope incidents become invisible to the team whose code caused them.
2. Findings tagged for one scope are missed by other consumers searching the same vector index, even though the underlying issue is identical.
3. Decisions that apply to shared code have no clear home — either they hide under one team's scope, or they land in an ambiguous "monorepo-wide" bucket with no review authority.

## Decision

Records carry three scope-related fields:

- `owningScope` — single scope that owns review authority for changes to the record. Resolves to a CODEOWNERS team for PR review.
- `affectedScopes[]` — list of other scopes the record is relevant to. Records appear in those scopes' `firetrail ready`, `firetrail board`, and `firetrail search` default-filtered results.
- `appliesTo[]` — optional list of path globs the record applies to. Used by `firetrail check pr` to detect when a PR touches code governed by an existing decision, and to detect overlap or conflict between decisions.

Cross-cutting records (e.g., a Redis cluster finding) declare a `home` scope as the owning authority — typically the team that owns the shared infrastructure — and list every consuming scope under `affectedScopes`.

A `monorepo-wide` scope value is forbidden. Records that genuinely apply to the entire monorepo must declare an explicit governance group in root config (e.g., `monorepoWide.reviewers: [@architecture-council]`) and require N-of-M review rather than single-team approval.

## Consequences

Positive:

- A finding about a payment-team change appears in payment-team's `ready` queue even when filed by checkout-team.
- A shared Redis finding is findable from every consuming team's scope without `--all-scopes`.
- Decisions that touch shared code surface conflicts at PR check time, not silently months later.
- CODEOWNERS authorization aggregates over `owningScope` plus any `affectedScopes` whose ownership pattern is touched.
- No record is orphaned at "monorepo-wide" without a declared review authority.

Negative:

- Schema is slightly more complex than a single string field. Worth it.
- Authoring records requires deciding the right `owningScope` vs `affectedScopes` split. The CLI offers heuristics — file paths in evidence and `appliesTo` globs suggest candidates — but human judgment is still required.
- Search-result ranking must weight by current-scope vs other-scope to avoid drowning users in cross-scope noise. Default ordering: own scope first, related scopes grouped, distant scopes last.

## Consequences for tooling

- `firetrail incident create` prompts for `affectedScopes` when the symptom path differs from the cause path.
- `firetrail check pr` resolves `appliesTo` globs and surfaces relevant decisions for any PR touching governed paths.
- `firetrail decision overlap` lists decisions whose `appliesTo` ranges intersect, flagging potential conflicts.
- `firetrail search` defaults to `--scope all` (with own-scope-first ranking) for the `search` and `similar` commands. Defaults to current-scope-only for `ready` and `board`, which are workflow tools rather than discovery tools.

## Alternatives considered

**Single `scope` label.** Original design. Disqualified by the failures above.

**Tags only, no scope field.** Loses the CODEOWNERS routing story. Tags are too informal to anchor review authority.

**Scopes as first-class records with relationships.** Cleaner conceptually but over-engineered for v1. Three fields on a record cover the actual use cases.

**Scope hierarchy (e.g., `apps/checkout/cache`).** Allowed inside scope values as a string convention. The system does not interpret hierarchy specially; it is decoration that helps humans scan.

## Addendum (2026-05-31): authoring `.firetrail/scopes.yaml`

The original ADR settled the record-side schema (`owningScope` / `affectedScopes` / `appliesTo`) and the read path: `ft-scope` compiles `.firetrail/scopes.yaml` into a registry that resolves a path to its owning scope and CODEOWNERS. What it left implicit was *how scopes get into that file*. They were hand-edited. This addendum records the decisions made when shipping a write surface (`ft_scope::writer`, `firetrail scope add|edit|rm|reorder`, the `/api/scope` mutations, and the scope-explorer UI).

**Resolution is last-declared-wins; declaration order is precedence.** A path can match more than one scope's `appliesTo` globs. The resolver takes the **last** matching scope in source order, identical to how firetrail already parses CODEOWNERS (last-match-wins). This gives the system *one* precedence model end-to-end — scope resolution, CODEOWNERS routing, and per-scope profile resolution (the spec below) all key on the same rule. The authoring convention follows from it: **declare broad scopes first, narrow exceptions last; a catch-all goes at the top.** `scope add` always appends (the new scope becomes last-declared, i.e. highest precedence); `scope reorder` is the explicit lever for changing precedence. We deliberately did not adopt ESLint-style most-specific / specificity-scored resolution — it is non-local, ambiguous for overlapping globs, and surprises everyone who reasons about it by reading top-to-bottom.

**Progressive disclosure: `scopes.yaml` is never auto-created, and a standalone repo pays nothing.** A missing file is not an error — it yields an empty registry, every path resolves with no owning scope, and the words "scope", "precedence", "shadow" never surface. `firetrail init` does not write the file. The UI's standalone empty state explains scopes are only for monorepos and offers an opt-in "Add a scope" (plus a suggest-only scaffold that proposes `<dir>/**` candidates from package directories — suggestions only, each confirmed by hand). Scopes are an opt-in monorepo concern; teams that do not need ownership boundaries never meet them.

**YAML round-trip: regenerate the block under a tool-managed header; hand comments are not preserved.** The writer is order-stable but it does not patch the file in place — on every save it re-serializes the whole `scopes:` block deterministically and prepends a fixed header comment (`# Managed by firetrail scope. Order matters: resolution is last-declared-wins.`). Validation (globs compile, unique ids, unique aliases across scopes, non-empty `appliesTo`) runs *before* the bytes touch disk, so an invalid model never lands. The cost is that hand-written comments inside the block are dropped. We accepted that for v1: the file is now tool-managed, the header says so, and the alternative (a comment-preserving YAML editor) is disproportionate to the value of free-text annotations on a generated routing table.

These three decisions are the substrate the per-scope-profiles design (`docs/specs/2026-05-31-per-scope-profiles-design.md`) sits on: profiles, CODEOWNERS routing, and multi-scope review all bind to a package through the same scope id and resolve through the same last-declared-wins rule. See `docs/components/scope-authoring.md` for the surface contract.

## References

- ADR-0002: JSON-in-Git storage
- ADR-0008: Identity registry (CODEOWNERS resolution depends on identity)
- `docs/components/scope-authoring.md` — the write-surface contract
- `docs/specs/2026-05-31-per-scope-profiles-design.md` — per-scope profiles built on this axis

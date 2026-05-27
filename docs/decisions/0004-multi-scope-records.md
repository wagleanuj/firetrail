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

## References

- ADR-0002: JSON-in-Git storage
- ADR-0008: Identity registry (CODEOWNERS resolution depends on identity)

# ADR-0008: Identity registry, not raw git email

## Status

Accepted — 2026-05-26

## Context

Firetrail records carry `created_by`, `claimed_by`, `reviewer`, `checked_by`, and audit-log actor fields. The naive resolution — `git config user.email` — fails at team scale in predictable ways:

- **Multi-machine engineers** have different git emails on laptop and desktop. Records they create on one machine cannot be matched to claims they made on another.
- **Offboarded engineers** leave dangling claims. Two hundred open tasks reference an email that no longer resolves. No one can release the claims.
- **Pair programming** records only one author. The `Co-authored-by` trailer is ignored.
- **Bots and CI runners** are indistinguishable from humans in the audit log. A PagerDuty webhook bot and an on-call engineer appear identical.
- **Fork-and-PR contributors** write records under personal email addresses that should not enter the canonical record.
- **CI running on behalf of a PR author** attributes checks to the CI runner identity rather than the author.
- **Two contractors sharing one email address** are indistinguishable.

Trust workflows collapse without a coherent identity model.

## Decision

Firetrail maintains an identity registry as a first-class part of the configuration. The registry resolves raw identifiers (git emails, GitHub usernames, SSO claims) to canonical identities. Every record write resolves to a canonical identity at write time and stores the canonical identifier, not the raw input.

### Registry shape

```yaml
# .firetrail/identity.yml  (or in external mode: identity.yml in the data repo)
identities:
  - id: alice
    kind: human
    status: active
    canonical_email: alice@company.com
    aliases:
      - alice@company.com
      - alice.smith@company.com
      - alice@personal.com
    github: alice-smith

  - id: pagerduty-bot
    kind: bot
    status: active
    canonical_email: pagerduty@company.com
    capabilities:
      can_create: [incident]
      can_review: false
      can_verify: false

  - id: ci-github-actions
    kind: ci
    status: active
    canonical_email: github-actions@github.com
    can_act_on_behalf_of: true
```

### Resolution order

1. `FIRETRAIL_AUTHOR` environment variable.
2. `firetrail` local config (`~/.firetrail/identity`).
3. `git config user.email` looked up against the registry's `aliases`.

If the resolved email is not present in the registry, behavior depends on the configured strict mode:

- `strict: true` — the record write fails with a clear error. Used by teams that want every record attributable.
- `strict: false` — the email is recorded as `external:<email>` and surfaced in audit views. Used during pilot and for external contributors.

### Identity kinds

- `human` — a real person. Default capabilities: create, claim, review, verify.
- `bot` — a service account. Default capabilities: create only. Cannot review or verify by policy.
- `ci` — a continuous integration runner. Can act on behalf of a human identity when the actor is known (resolved from `GITHUB_ACTOR`, `GITLAB_USER_LOGIN`, or equivalent). Audit log records both `actor: ci-runner` and `on_behalf_of: alice`.

### Capability matrix

The registry enforces who can perform which actions. The defaults above are overridable per identity. Bots cannot promote memory to `verified`. CI runners cannot promote memory to `verified` on their own — they require an `on_behalf_of` claim with a signed token from the upstream system.

### Co-authorship

Actor fields accept either a single identity or an array. `firetrail` reads the staging commit's `Co-authored-by` trailers and includes co-authors automatically. A `--with @carol` flag is also supported.

### Status lifecycle

Identities have a `status` field: `active`, `offboarded`, `disabled`. Offboarded identities cannot hold live claims. A sweep job (`firetrail identity sweep`) releases claims held by offboarded identities and creates an audit-log entry per release.

A claim's `claim_expires_at` field is mandatory at write time (default seven days). On expiry, the claim is automatically released and the record returns to the `ready` queue.

### Reassignment

`firetrail claim takeover <record-id> --reason <text>` releases a stale claim, assigns to the current actor, and writes a `reassigned_from` field plus an audit entry.

### Tamper-evidence at the registry

The identity registry is itself a record file (or a small set of them) version-controlled by Git. Changes to the registry follow the same PR review process. CODEOWNERS gates registry edits to a designated identity-admin team.

## Consequences

Positive:

- Multi-machine engineers resolve to one canonical identity.
- Offboarding is operational, not aspirational — the sweep job clears zombie claims automatically.
- Bots and humans are visually and behaviorally distinct in audit views and capability checks.
- CI runners are honest about acting on behalf of humans, and the on-behalf-of relationship is auditable.
- Pair programming attribution survives. Records carry the contributor set, not just the typist.
- External contributors are tagged but not blocked, with clear default policy.

Negative:

- The registry is a new artifact to maintain. Onboarding now includes adding an identity entry. Mitigated by tooling — `firetrail identity add` and bulk import from GitHub team membership.
- Strict mode at adoption can block writes when a new hire's email is not yet registered. Acceptable trade — the alternative is permanently undiscoverable provenance.

## Alternatives considered

**Raw git email everywhere.** Disqualified by the failures listed in Context.

**GitHub usernames as the canonical identifier.** Tightly couples Firetrail to GitHub. Teams using GitLab, Bitbucket, or self-hosted Git would have a worse experience. Rejected as primary; GitHub username is one of several alias types.

**SSO claims as the canonical identifier.** Cleanest in theory but requires every team to have SSO and to wire its tokens into the CLI. Out of scope for v1. Supported as an alias source for teams that have it.

**One identity, one email.** Disqualified by the multi-machine and multi-context realities.

## References

- ADR-0004: Multi-scope records (CODEOWNERS authorization depends on identity)
- ADR-0006: Storage modes (external mode hosts the identity registry in the data repo)

# ADR-0013: Trust model — states, evidence, origin, and risk class

## Status

Accepted — 2026-05-26

## Context

Memory records (findings, decisions, runbooks, gotchas, memory) drive future behavior. Future engineers and future AI agents read them and act on them. If the trust state of a record can be elevated without rigor, the system poisons its own future.

Three concrete failure modes from the stress-testing:

1. **Trust laundering.** A draft finding becomes "reviewed" because a busy human approved a JSON diff in a PR. PR approval is not the same epistemic act as verifying a technical claim.

2. **Agent override of verified memory.** An AI agent unilaterally deprecates a verified finding based on a transient symptom. Verified-trust memory is wiped without a second human ever seeing the decision.

3. **Memory poisoning.** A subtly wrong finding ("disable rate limiting in production during high traffic") slips through review. Future agents read it during priming and apply the advice.

The trust model must encode *how* a claim was verified, not just *whether*. It must distinguish AI-generated content from human-generated content even after promotion. It must protect high-stakes domains (security, availability, data loss) more aggressively than low-stakes ones.

## Decision

### Trust states

```
draft       — new record; default state for any created record
reviewed    — at least one human has read it and explicitly approved with evidence
verified    — a second human has independently confirmed; high trust
deprecated  — superseded or no longer believed correct; kept for audit
archived    — closed lifecycle, not displayed in default queries
stale       — automatic state for records past review-due date without refresh
rejected    — explicitly rejected during review; kept for audit
superseded  — replaced by a newer record (with pointer to successor)
redacted    — content removed for security; metadata retained
```

### Promotion requirements

- `draft → reviewed`: requires `firetrail review <id> --evidence <ref>` with a non-empty evidence link. PR approval alone is insufficient. The evidence link must point to an artifact (commit, log query, test run, dashboard, incident report, manual validation note) that supports the claim.
- `reviewed → verified`: requires a *second* human identity (not the author of the PR that introduced the record) to run `firetrail memory promote <id> --verified`. Single-person verification is forbidden.
- Any transition into `deprecated` for a `verified` record requires either:
  - A human identity acting, and within 7 days a second human acknowledging via `firetrail memory ack-deprecation <id>`. Without the ack, the deprecation auto-reverts.
  - Or a `supersedes:` pointer to a newer record that takes the place of the deprecated one.

Agents (any identity with `kind: bot` or `kind: ci`) cannot directly transition records into `verified`. Agents may file a `verification proposal` record that a human acts on.

### Origin flag

Every record carries a permanent `origin: human | agent | imported` field set at creation:

- `human`: a human identity created the record via interactive CLI.
- `agent`: an AI agent created the record (identity kind is `bot` or the actor's session metadata flags the call as agent-driven).
- `imported`: the record came from a bulk import. Subject to additional quarantine rules (ADR-0014).

The origin flag persists across promotions. A verified-by-two-humans record that was originally `agent` keeps `origin: agent` forever. The flag is surfaced in `prime` output and in audit views so reviewers can see the provenance.

### Risk class

Records carry an optional `risk_class` field with values:

```
security      — security posture, authentication, authorization, secrets
availability  — uptime, latency, capacity, failover
data-loss     — durability, backups, retention, integrity
compliance    — regulatory, legal, contractual
performance   — speed, efficiency, cost (lower stakes than availability)
correctness   — functional bugs (lower stakes)
```

A record with `risk_class: security`, `availability`, `data-loss`, or `compliance` is high-stakes. High-stakes records require:

- `verified` status before they appear in `prime` output by default.
- A linked test, postmortem, or production validation as evidence.
- Re-validation every 180 days (configurable). Past the date, status auto-transitions to `stale`.

Low-stakes records follow standard trust rules without the additional requirements.

### Draft hygiene

Drafts left untouched expire after 14 days (configurable) and either auto-deprecate or surface in a `firetrail memory stale` report depending on team policy. Agents tend to create many drafts; without expiry the corpus accumulates noise that contaminates search.

### Acceptance criteria hygiene

A task may have at most 10 acceptance criteria by default (configurable). Agent-created criteria are flagged `proposed` and must be explicitly confirmed by a human before they become blocking. This prevents the AC-spam failure mode where an agent generates 47 criteria and the task becomes uncloseable.

### Prime output rules

`firetrail prime` by default includes:

- `verified` records of any risk class.
- `reviewed` records of low-stakes risk class.

Excludes by default:

- `draft`, `stale`, `deprecated`, `rejected`, `superseded`, `redacted`, `archived`.
- High-stakes records that are `reviewed` but not yet `verified`.
- Records flagged `origin: agent` that have not been human-reviewed at minimum.

Override flags (`--include-drafts`, `--include-stale`) are explicit and noisy in output.

## Consequences

Positive:

- Trust earned requires the work the level implies. "Reviewed" means a human reviewed with evidence. "Verified" means two humans agree.
- AI-generated content is permanently marked, even after human review. Reviewers and consumers see what they are reading.
- High-stakes domains have stricter rules without imposing them on the long tail of low-stakes records.
- Drafts auto-expire instead of accumulating as latent landmines.
- AC spam is bounded.
- Agents cannot silently override verified memory.

Negative:

- More fields on each record. Schema complexity worth the trust gains.
- Promotion friction is real — getting a record to `verified` requires two humans engaging with it. Acceptable; that is the point.
- Risk-class assignment requires human judgment at creation time. Tooling can suggest a default based on keywords, but humans confirm.

## Alternatives considered

**Simpler model: just draft / approved.** Disqualified by the AI-scale failure modes. Without the second-reviewer and origin-flag protections, agent-authored memory poisons the corpus.

**Trust transitions purely automated.** Disqualified by trust laundering — automation that promotes on PR-approval reduces "reviewed" to "someone clicked merge."

**Per-team trust policies fully configurable.** Possible later. v1 ships the rules above as defaults and allows narrow per-scope overrides (review window, draft expiry days) but not the core requirements.

## References

- ADR-0008: Identity registry (capability matrix for who can do what)
- ADR-0014: Import quarantine (additional rules for imported origin)
- ADR-0009: Memory-only PRs (the review workflow)

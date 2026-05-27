# ADR-0019: Prime output respects a context budget and discloses omissions

## Status

Accepted — 2026-05-26

## Context

`firetrail prime` produces a context pack that an AI agent (or a human) reads before working on a task or responding to an incident. For a mature record set, the pack can easily exceed any reasonable context budget — 47 related findings, 12 incidents, 8 runbooks, 5 decisions, multiple PR histories, suggested files.

Agents have hard token limits. A pack that exceeds the limit is silently truncated by the host system, and the agent makes decisions on the truncated subset without knowing what was cut. This is the worst kind of failure — confident action on incomplete information.

We need prime to do three things:

1. Stay within a configurable token budget.
2. Be deterministic — the same query produces the same output, so agents can reason about completeness.
3. Disclose what was omitted, so the agent can ask for more or warn the human.

## Decision

### Configurable token budget

`firetrail prime` accepts `--max-tokens <n>`. Defaults:

- Markdown format: 8,000 tokens.
- JSON format: 16,000 tokens.

The defaults are calibrated for current frontier models' typical task-context window; teams can override per their model and use case.

### Deterministic prioritization

Prime walks candidate context in priority order. The order is fixed and documented:

1. The current record itself (task, incident, etc.) in full.
2. Acceptance criteria of the current record, in full.
3. Direct relations of the current record (linked incident, finding, runbook, decision), in full.
4. `verified` memory matching the current record's scope and topic.
5. `reviewed` memory matching the current record's scope and topic, by recency.
6. Suggested files (paths mentioned in evidence or appliesTo globs).
7. Indirect relations (relations-of-relations) — bounded to one hop.
8. Lower-confidence matches by vector similarity.

Within each priority level, order is deterministic by `(scope_distance, recency, score)`.

### Token accounting

Token counts are computed using a model-appropriate tokenizer. For the default markdown format, Firetrail ships a lightweight estimator that approximates token counts to within ~5%. Teams that need exact counts can configure an explicit tokenizer (e.g. `tiktoken` via shell-out, or a bundled Rust tokenizer).

### Truncation policy

When adding the next item would exceed the budget:

- Items in priority levels 1–3 are never truncated. They are required context. If they alone exceed the budget, the budget is overridden upward with a notice.
- Items in levels 4+ are dropped (not partially included). Partial inclusion is more dangerous than omission — the agent sees half a finding without knowing it is half.
- The decision to drop is deterministic given the priority order.

### Disclosure: the `omitted` manifest

Prime output includes an explicit `omitted` section listing what was excluded:

```markdown
## Omitted from this context pack (would exceed budget)

The following records matched but were not included. Run with a larger
`--max-tokens` or use the listed IDs to fetch directly.

- FIND-9c4b2e [reviewed, 0.78 sim] Redis client pool sizing under load
- FIND-7a3c1d [reviewed, 0.71 sim] Connection pool exhaustion in worker B
- RUN-311abc [verified] Inspect Redis pool saturation
- 8 more findings with similarity 0.6–0.7
- 3 indirect relations of INC-2481

Total omitted: 14 records (~6,200 tokens).
```

In JSON format the `omitted` field is structured:

```json
{
  "omitted": [
    {"id": "FIND-9c4b2e", "type": "finding", "trust": "reviewed", "score": 0.78, "reason": "budget"},
    ...
  ],
  "omitted_summary": {
    "count": 14,
    "estimated_tokens": 6200,
    "by_type": {"finding": 9, "runbook": 1, "indirect_relation": 3, "decision": 1}
  }
}
```

### Skill instruction

The Claude Code skill (ADR-0012) explicitly instructs the agent: if the `omitted` section is non-empty and the agent's decision depends on context that might be in those records, the agent must either request a larger budget, fetch specific omitted records by ID, or warn the human that the context is incomplete.

### Trust filtering

Prime's default trust filter (ADR-0013): include `verified` of any risk class plus `reviewed` of low-stakes risk class. `draft`, `stale`, `deprecated`, `rejected`, `superseded`, `redacted`, `archived` are excluded.

Override flags:

- `--include-drafts` includes drafts (with `[draft]` tags in output).
- `--include-stale` includes stale records (with `[stale]` tags).
- `--all-trust` includes every state including deprecated.

These flags annotate the output prominently so the agent knows the trust posture has been relaxed.

## Consequences

Positive:

- Prime fits within a known budget. Agents do not silently lose context to truncation.
- Determinism: same query produces the same output, so the agent can reason about whether to ask for more.
- The `omitted` manifest makes incompleteness visible. The agent — or a human reviewing the agent's work — can act on it.
- Priority order is documented, so engineers understand what they get and what they do not.

Negative:

- Token estimation is approximate unless an exact tokenizer is configured. Acceptable — the budget is a target, not a hard ceiling, and a 5% approximation error is within natural agent variance.
- The `omitted` manifest occupies its own tokens. Mitigated by including only IDs and minimal metadata, summarized when long.

## Consequences for design

- `firetrail prime` is part of the `ft-prime` crate and depends only on the index, the trust filter, and the priority logic.
- Output formatters (markdown, json, toon if requested in future) share the same priority logic; only the rendering differs.
- The skill ships with explicit instructions on how to respond to a non-empty `omitted` manifest.

## Alternatives considered

**Silent truncation (the failure mode).** Disqualified — the whole point of this ADR.

**No budget, dump everything.** Breaks agent context windows. Disqualified.

**Per-priority-level partial truncation (include half a finding).** More confusing than dropping the whole finding. Partial context is the worst kind. Disqualified.

**Always include the agent's previous prime output for continuity.** Bloats budget across sessions and assumes session state. Out of scope for v1.

## References

- ADR-0013: Trust model (the trust filter)
- ADR-0012: Skill as agent docs (how the agent is instructed to handle omissions)
- ADR-0005: No LLM calls from the tool itself (prime is the bridge to the host agent)

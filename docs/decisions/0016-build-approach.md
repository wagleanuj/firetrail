# ADR-0016: Build approach — cargo workspace, parallel subagents, layered test harness

## Status

Accepted — 2026-05-26

## Context

Firetrail will be built by one human plus AI subagents. This is an unusual development environment: parallelism is cheap (many agents can run in parallel), but each agent has limited context, no continuity across sessions, and a tendency to confidently claim "done" on broken code.

We need a build workflow that:

1. Decomposes the system into chunks small enough for one agent's context.
2. Validates each chunk independently before integration.
3. Runs validation fast enough to support tight iteration.
4. Catches the failure modes that AI-written code typically exhibits — missed edge cases, hallucinated APIs, broken integration with code written by other agents, false claims of completeness.

## Decision

### Workspace structure

Firetrail is a Cargo workspace with many small crates rather than one large crate. Each crate is a coherent unit of context for one agent, typically 2,000–5,000 lines.

```
firetrail/
├── Cargo.toml                  # workspace root
├── crates/
│   ├── ft-core/                # record types, schema, hash chain
│   ├── ft-storage/             # JSON-in-Git read/write, embedded + external
│   ├── ft-index/               # SQLite + sqlite-vec read index
│   ├── ft-embed/               # ONNX daemon, embedding cache
│   ├── ft-identity/            # registry, resolution, capabilities
│   ├── ft-scope/               # multi-scope routing, CODEOWNERS resolution
│   ├── ft-trust/               # trust state machine, evidence, review workflow
│   ├── ft-history/             # PR-time compaction, prev_state_hash chain
│   ├── ft-search/              # vector + lexical + ranking
│   ├── ft-prime/               # context pack generation
│   ├── ft-pr/                  # check pr, custom merge driver
│   ├── ft-import/              # markdown, jira, confluence importers
│   ├── ft-git/                 # git operations wrapper
│   ├── ft-cli/                 # clap entry, command dispatch
│   └── ft-testkit/             # shared test fixtures, factories
└── tests/
    └── scenarios/              # end-to-end black-box tests
```

Why this matters in an agent context:

- Each crate fits in one agent's working memory.
- Incremental compile rebuilds only the changed crate.
- Public APIs between crates are explicit. Agents cannot accidentally couple to internals because they cannot see them.
- Tests target one crate without bringing the world.

### Five-layer test harness

Test layers from fastest to slowest. Inner-loop development depends on the first three; CI runs all five.

#### Layer 0: Compile (instant)

The Rust compiler is the first validator. Trust state machines, scope routing, record kinds, and identity capabilities are encoded as `enum` and type-state where possible so that incorrect transitions are compile errors rather than runtime bugs. Every type invariant we can encode into types removes a class of tests.

#### Layer 1: Unit tests (sub-second per crate)

Pure logic, no filesystem, no SQLite file, no Git. Per-module. Run on every save with `cargo nextest`.

#### Layer 2: Property tests (seconds)

`proptest` over structured inputs — record parsing, scope routing logic, merge driver, hash chain validation, trust transitions. Property tests catch the edge cases AI agents predictably miss: empty arrays, deeply nested structures, unicode, conflicting fields, malformed inputs.

#### Layer 3: Integration tests (seconds to a few minutes)

Real SQLite (in tempfile), real Git repos (in tempdir), real filesystem. `ft-testkit` provides `TestRepo::new()` that returns an isolated workspace per test.

#### Layer 4: Scenario tests (a few minutes)

Black-box CLI tests. Spawn the `firetrail` binary against a fixture repo, execute a sequence of commands, assert observable state. Each scenario is a YAML or RON file:

```yaml
# tests/scenarios/incident-lifecycle.scenario
setup: empty_embedded_repo
steps:
  - cmd: firetrail incident create "checkout latency" --service checkout-api
    expect:
      record_exists: incident/INC-*
      record_field: { status: open, owningScope: apps/checkout }
  - cmd: firetrail finding create "redis pool exhaustion" --incident "$LAST_INCIDENT"
    expect:
      record_exists: finding/FIND-*
```

The scenario runner lives in `ft-testkit`.

#### Layer 5: Conflict and merge tests (a few minutes)

Two engineers, two branches, conflicting edits, the custom merge driver runs, the expected final state is asserted. Force-push detection, rebase preservation, squash compaction, branch salvage on deletion. These tests catch the concurrency bugs the AI will absolutely miss.

### Parallel subagent workflow

Components form a dependency DAG. We build in waves; within each wave, crates are independent and can be implemented by separate subagents in parallel git worktrees.

```
Wave 1 (foundation):
  ft-core, ft-git, ft-testkit

Wave 2 (parallel after Wave 1):
  ft-storage, ft-identity, ft-history

Wave 3 (parallel after Wave 2):
  ft-index, ft-embed, ft-scope, ft-trust

Wave 4 (parallel after Wave 3):
  ft-search, ft-prime, ft-import, ft-pr

Wave 5:
  ft-cli (glue layer)
```

Each subagent receives:

1. The component spec from `docs/components/<name>.md`.
2. Relevant ADRs linked from the spec.
3. The crate skeleton with public API stubbed.
4. A list of required tests that must pass.
5. A constraint: do not modify other crates' code.

The agent implements until the test list passes. It is free to add tests if it finds gaps.

### Verifier-agent pattern

For every component PR, a second subagent runs as an independent reviewer:

> You did not write this code. Read the spec, the ADRs, and the diff. Without consulting the author's tests, write three additional tests the implementation should pass. Run them. Report results.

This catches the AI-builds-AI-validates failure mode. Implementer and verifier have different prompts and therefore make different mistakes.

### Validation gates per PR

Every PR must pass before merge:

1. `cargo fmt --check`.
2. `cargo clippy -- -D warnings`.
3. `cargo nextest run` (unit + property + integration).
4. `cargo test --doc`.
5. Scenario suite passes.
6. Verifier subagent signs off in writing on the PR.
7. Spec-adherence: `docs/components/<name>.md` requirements are demonstrably covered by tests.

A local pre-commit hook runs items 1–3. CI runs all of them on every PR.

### Speed strategies

| Strategy | Impact |
|---|---|
| `cargo nextest` for parallel test execution | 5–10× faster than `cargo test` |
| `sccache` for compile caching | 3–5× faster cold builds |
| `mold` linker on Linux | ~30% faster link times |
| Many small crates | incremental compile touches only changed crate |
| `cargo-watch` during dev | live feedback loop |
| In-memory SQLite for unit tests | no disk I/O |
| Mocked embedding daemon by default in tests | no model load |
| Feature-gated slow tests (`#[cfg(feature = "slow-tests")]`) | inner loop stays fast; CI runs everything |

Target loop times:

- Compile + unit tests on one crate: under 5 seconds.
- Full unit + property suite (all crates): under 30 seconds.
- Integration suite: under 2 minutes.
- Scenario suite: under 5 minutes.
- Full validation including conflict/merge tests: under 10 minutes.

## Consequences

Positive:

- Parallelism scales with available agent capacity. Wave 3 can have four agents working at once.
- Validation is layered and fast. The compiler catches the most bugs, fastest. Tests catch the rest.
- Component boundaries are real because they are crate boundaries.
- Verifier-agent pattern catches the "agent claims done on broken code" failure mode at PR time, not at integration time.
- Specs in `docs/components/` are the load-bearing input. Writing them well is the highest-leverage activity before code.

Negative:

- Front-loaded design work. Component specs must be detailed enough that an agent can implement without ambiguity. This investment is paid for once and amortized across all parallel work.
- More small files than a monolithic codebase. Cargo workspaces handle this cleanly; navigation cost is offset by clear boundaries.
- Wave 1 must be built carefully and somewhat sequentially because Waves 2–5 depend on it. The first wave is the most demanding.

## Implementation order

1. Finish ADRs (this and remaining decisions).
2. Write `docs/ARCHITECTURE.md`.
3. Write `docs/components/<name>.md` for each crate. **This is the highest-leverage step.**
4. Set up cargo workspace with crate skeletons.
5. Build Wave 1 (ft-core, ft-git, ft-testkit) carefully — this validates the workflow on a small surface.
6. Build Waves 2–5 in parallel using the subagent pattern proven in Wave 1.
7. Wire the integration scenarios and conflict tests.
8. Ship MVP.

## Alternatives considered

**Monolithic crate.** Disqualified by the context-window constraint and by losing incremental compile benefits.

**Test only at the integration level.** Slow feedback. AI agents need fast feedback to converge. Layered testing wins on speed and on diagnostic clarity.

**Skip the verifier-agent pattern.** Disqualified by the AI-claims-done-on-broken-code failure mode. The verifier is cheap insurance.

**Specs as light scaffolding, agents fill in design.** Tried implicitly in our brainstorming. Disqualified — different agents diverge on interpretation, integrations break, total rework cost exceeds the upfront spec investment.

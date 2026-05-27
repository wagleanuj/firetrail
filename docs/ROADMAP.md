# Firetrail Roadmap

This document is the strategic plan: what we ship, in what order, with what gates between
milestones. Each milestone is independently shippable — usable on its own and adds enough
value that a team would adopt at that point if no further milestone existed.

Implementation work is tracked in beads as epics and tasks. This document is the
narrative; beads is the executable plan.

---

## Vision for v1.0

A team installs Firetrail. From day one they can:

- Track tasks, bugs, subtasks, and epics with dependencies and acceptance criteria.
- Capture incidents, findings, runbooks, decisions, and gotchas as structured records.
- Search the resulting corpus semantically and by metadata.
- Prime AI coding agents with task-relevant context.
- Enforce review of memory changes through PRs.
- Import existing markdown incident archives into a quarantine for promotion.
- Operate in either single-repo or multi-repo configurations.

All of this works offline, requires no LLM API key, distributes as a single static
binary, and stays correct under concurrent multi-branch work.

---

## Milestones

Six milestones from foundation to v1.0. Each one is shippable.

```
M1 Local Work Graph         → engineers can manage work, dependencies, criteria
M2 Incident Memory          → engineers can capture and review production knowledge
M3 Search + Prime           → corpus becomes discoverable; agents get useful context
M4 PR Safety + CI           → memory governance becomes enforced, not aspirational
M5 Multi-scope + Team       → monorepos and multi-repo orgs are first-class
M6 Importers                → historical knowledge enters the graph safely
```

Crate dependency graph for the milestones (see ADR-0016):

```
Wave 1 (foundation):    ft-core, ft-git, ft-testkit
Wave 2 (parallel):      ft-storage, ft-identity, ft-history
Wave 3 (parallel):      ft-index, ft-embed, ft-scope, ft-trust
Wave 4 (parallel):      ft-search, ft-prime, ft-import, ft-pr
Wave 5:                 ft-cli (glue)
```

Each milestone draws from one or more waves. Milestones do not map one-to-one to waves —
M1 needs partial coverage of waves 1–3 plus the relevant slice of ft-cli.

---

## M1 — Local Work Graph

**Goal.** A single engineer can run `firetrail init`, create epics and tasks, declare
dependencies, add acceptance criteria, claim work, and close work. The board view shows
state. The graph view shows blockers. Everything is JSON-in-Git, validated, and
recoverable from a rebuilt index.

**Shippable promise.** Replace lightweight personal task tracking on a single repo. Not
yet competitive with full team tools (no memory, no search, no PR enforcement).

**Crate coverage.**
- `ft-core` — full record schema for `task`, `subtask`, `bug`, `epic`, plus
  `acceptance_criterion` and `evidence`.
- `ft-git` — git operations wrapper.
- `ft-testkit` — `TestRepo` fixture, factories for records, scenario runner skeleton.
- `ft-storage` — embedded mode only. JSON read/write under `.firetrail/records/`.
  External mode deferred to M5.
- `ft-identity` — basic resolution from `git config user.email`. Full registry deferred
  to M5.
- `ft-index` — SQLite schema, basic queries (`list`, `ready`, dependency walk).
  Vector index deferred to M3.
- `ft-cli` — `init`, `doctor`, `task`, `epic`, `subtask`, `bug`, `criteria`,
  `dep`, `claim`, `unclaim`, `close`, `ready`, `board`, `graph`, `link`, `show`,
  `list`.

**Success criteria.**

- A scripted scenario walks through: init → epic create → 3 tasks with dependencies →
  acceptance criteria added → tasks claimed and closed in dependency order → board
  reflects state at each step. Runs in under 30 seconds.
- A second clone of the same repo runs `firetrail` commands without rebuilding anything;
  the index reconstructs on first command.
- Conflict scenario: two branches add tasks under the same epic; merge succeeds via the
  JSON merge driver; both tasks present on main; index reflects both.
- `firetrail close` enforces acceptance-criteria completion; `--force` works with a
  recorded reason.
- All Layer 0–3 tests pass for the listed crates. Scenario tests for M1 pass.

**Gates.**

- 100% of M1 acceptance criteria covered by tests.
- Verifier subagent has signed off on each M1 component PR.
- `firetrail doctor` reports a clean repo after the scripted scenario.
- M1 binary distributes as a single executable for macOS (arm64, x86_64) and Linux
  (x86_64). Windows deferred to M4.

---

## M2 — Incident Memory

**Goal.** A team can record findings, runbooks, decisions, gotchas, and incidents.
Memory records have trust states. Memory-only PRs are enforced for findings, decisions,
incidents, and runbooks (ADR-0009). The history chain (ADR-0003, ADR-0017) is functional.
Branch salvage (ADR-0018) catches abandoned-branch loss.

**Shippable promise.** A team can stop using ad-hoc markdown postmortems and Confluence
runbook pages for *new* knowledge. Search (M3) is what makes the corpus discoverable,
but M2 alone gives reviewable, structured, version-controlled memory.

**Crate coverage.**
- `ft-history` — PR-time compaction, `state_hash`/`prev_state_hash`, `firetrail verify`.
- `ft-trust` — trust state machine (`draft → reviewed → verified`), evidence
  requirements, `origin` flag, risk-class taxonomy.
- `ft-core` extensions — `incident`, `finding`, `runbook`, `decision`, `gotcha`,
  `memory` types.
- `ft-storage` extensions — memory-only PR detection on commit, history append/compact.
- `ft-cli` — `incident`, `finding`, `runbook`, `decision`, `gotcha`, `memory`,
  `capture`, `verify`, plus `memory` lifecycle commands (`review`, `promote`,
  `deprecate`, `archive`, `supersede`, `merge`, `redact`).
- Salvage workflow — `firetrail memory salvage`, `post-checkout` and `post-merge`
  hooks installed by `init`.

**Success criteria.**

- Scenario: incident created on a feature branch, finding added, both auto-promote to
  main via memory-only PR; engineer continues feature work; finding visible to other
  team members immediately.
- Trust transition scenario: a finding cannot reach `verified` without a second
  reviewer; agents cannot promote `verified` records.
- Force-push scenario: simulate a force-push that rewrites a record on main; `firetrail
  verify` detects the broken `prev_state_hash` chain.
- Branch salvage scenario: a branch with 3 findings and 1 task is deleted; salvage
  prompts for the findings (default Y), the task (default N); a memory-only PR opens
  for the findings.
- Squash-merge scenario: a PR with 12 record-touching commits squash-merges to main; the
  compacted `history[]` entry on main correctly reflects PR-level provenance.

**Gates.**

- All M1 gates remain green.
- M2 acceptance criteria covered by tests.
- Layer 5 (conflict and merge) tests pass for the new record types.
- Documentation of the trust state machine in `docs/components/ft-trust.md` matches
  the implementation exactly.

---

## M3 — Search + Prime

**Goal.** The corpus becomes discoverable. Vector embeddings via local ONNX (ADR-0007)
plus lexical fallback. Hybrid search ranks results by similarity, recency, trust, and
scope distance. `firetrail prime` generates context packs respecting a token budget with
an `omitted` manifest (ADR-0019). The corpus becomes useful to AI coding agents in the
host session.

**Shippable promise.** A team's accumulated incident knowledge is searchable. AI agents
working in the repo can prime themselves with relevant context. The product loop —
capture knowledge, search it later, hand it to an agent — is now closed.

**Crate coverage.**
- `ft-embed` — ONNX runtime, `bge-small-en-v1.5` default, single-daemon architecture
  with Unix-socket queue, content-hash cache, integrity checksums.
- `ft-search` — vector search via sqlite-vec, BM25 fallback, hybrid ranking, scope
  filters, trust weighting.
- `ft-prime` — context pack generation, deterministic prioritization, token-budget
  accounting, omitted manifest, markdown + JSON formats.
- `ft-cli` — `search`, `similar`, `prime`, `index rebuild`, `index refresh`,
  `daemon start|stop|status`, embedding-related `doctor` checks.

**Success criteria.**

- Embedding round-trip: a finding is created, embedded asynchronously, becomes
  searchable within 1 second of write completion.
- Hybrid search returns expected results for fixture corpora; trust weighting prefers
  verified over reviewed over draft.
- `prime --task <id>` produces context within the configured budget; truncation
  populates the `omitted` manifest; `--max-tokens` adjusts the budget.
- Daemon survives multiple concurrent CLI processes; no SQLite write contention.
- Model upgrade scenario: switch from `bge-small-en-v1.5` to `bge-base-en-v1.5`;
  migration runs once, produces a downloadable cache artifact.
- Lexical fallback works when ONNX inference is unavailable; search results carry
  `mode: lexical` marker.

**Gates.**

- All M1, M2 gates remain green.
- M3 acceptance criteria covered by tests.
- Embedding cache integrity tests pass (silent corruption is detectable).
- `firetrail prime` produces deterministic output for the same query.
- Performance gate: scenario suite still under 5 minutes despite embedding workload.

---

## M4 — PR Safety + CI

**Goal.** Memory governance becomes enforced rather than convention. `firetrail check pr`
validates PRs against the full rule set (ADR-0009, ADR-0010, ADR-0013, ADR-0017). A
GitHub Action template ships. The custom JSON merge driver lands. Acceptance-criteria
enforcement, evidence requirements, secret scanning, and AC caps all run in CI.

**Shippable promise.** A team can confidently roll Firetrail out across multiple
engineers. Memory pollution, trust laundering, and provenance loss become structurally
impossible rather than politely discouraged.

**Crate coverage.**
- `ft-pr` — full `check pr` implementation, all validation rules, secret scanning,
  AC-cap enforcement, evidence resolution, draft auto-expiry detection, deprecated-
  reference detection.
- JSON merge driver — deterministic three-way merge for record JSON with array-order
  and criteria-list special cases.
- CI templates — GitHub Actions workflow, configurable strictness, comment-summary
  posting.
- `ft-cli` — `check`, `check pr`, `diff`, `diff --memory`, `lint memory`, `review`.
- Force-push protection — server-side pre-receive hook (shipped as configurable
  artifact), client-side PR-time check.

**Success criteria.**

- A PR that closes a task with incomplete acceptance criteria fails `check pr`.
- A PR that adds a finding without evidence fails `check pr`.
- A PR that mixes code changes with new findings fails `check pr` (ADR-0009).
- A PR with 200 record changes runs through `check pr` in under 60 seconds (content-
  hash caching for unchanged records).
- Secret-scanning catches simulated API keys in record content.
- The merge driver handles concurrent edits to acceptance criteria arrays without
  losing entries.
- A simulated force-push that rewrites `.firetrail/records/` is rejected by the
  pre-receive hook and detected by the PR-time check.

**Gates.**

- All M1–M3 gates remain green.
- M4 acceptance criteria covered by tests.
- GitHub Action runs cleanly against a fixture repo end-to-end in CI.
- Conflict-and-merge test suite (Layer 5) covers every documented edge case.

---

## M5 — Multi-scope + Team

**Goal.** Multi-scope records (ADR-0004), full identity registry (ADR-0008), per-scope
config overrides, CODEOWNERS-driven authorization, pilot rollout via `enabledScopes`,
and external storage mode (ADR-0006). Real team scale.

**Shippable promise.** Multi-team monorepos and multi-repo microservice orgs adopt
Firetrail without the workarounds. Pair programming, multi-machine engineers,
offboarding, and CI-on-behalf-of all work correctly.

**Crate coverage.**
- `ft-scope` — full multi-scope routing, CODEOWNERS parsing and resolution, scope
  aliasing, `appliesTo` glob matching, conflicting-decision detection.
- `ft-identity` — registry implementation, capability matrix, on-behalf-of, claim
  expiry and takeover, co-authorship via `Co-authored-by`, offboarding sweep.
- `ft-storage` — external mode: clone management at `.firetrail/cache/`, push/pull,
  PR-link enforcement, sync policy (`loose` shipped).
- `ft-cli` — `identity` subcommands, `claim takeover`, `scope` subcommands, init flow
  enhancements for storage-mode selection.

**Success criteria.**

- Multi-scope incident scenario: incident in `apps/checkout` with a finding affecting
  `apps/payment`; payment team's `ready` queue surfaces the finding; both teams' review
  required.
- External mode scenario: two code repos point at one data repo; finding in one repo's
  scope is discoverable from the other; PR-link enforcement validates references.
- Multi-machine engineer scenario: same canonical identity resolves from two different
  git emails; claim from machine A is closable from machine B.
- Offboarding scenario: a registered identity transitions to `offboarded`; the sweep
  job releases all live claims; audit log records each release.
- Pilot-rollout scenario: a monorepo with `enabledScopes: [apps/checkout]` runs
  `check pr` for checkout PRs only; other teams' PRs are unaffected.

**Gates.**

- All M1–M4 gates remain green.
- M5 acceptance criteria covered by tests.
- External-mode scenarios pass for the `loose` atomicity policy.
- Identity registry edits flow through their own PR review.

---

## M6 — Importers

**Goal.** Historical knowledge enters the graph safely. Markdown incident reports, ADRs,
runbooks, Jira issues, and Confluence pages import into a quarantine index (ADR-0014).
Promotion to the canonical index is explicit, gated, and auditable.

**Shippable promise.** A team with years of accumulated postmortems can bring them into
Firetrail without poisoning search or laundering trust. Imports become a resource, not
a firehose.

**Crate coverage.**
- `ft-import` — markdown parser with section detection (Symptoms, Root Cause,
  Resolution, Action Items, Lessons Learned), ADR importer, runbook importer.
- Jira MCP adapter — read issues by key, import linked records.
- Confluence MCP adapter — read pages, import postmortems and runbooks.
- Quarantine index — separate `embeddings_quarantine` table in SQLite, default
  exclusion from search and prime.
- Promotion workflow — `promote-import` interactive and batched modes, inbound-
  reference auto-candidacy.
- `ft-cli` — `import incidents`, `import adrs`, `import runbooks`, `import confluence`,
  `jira import`, `promote-import`, `import --refresh`.

**Success criteria.**

- Dry-run import of a 100-file markdown directory produces a quality report; no records
  are written.
- Apply import writes records to the quarantine; canonical search excludes them by
  default.
- `--include-quarantine` flag surfaces quarantine results explicitly labeled.
- Promotion scenario: a quarantined record gets three inbound references from canonical
  records; it becomes a promotion candidate; interactive promotion moves it to
  canonical.
- Re-import scenario: source markdown is edited; `import --refresh` updates quarantine
  records in place at stable IDs.
- Bulk-cap scenario: an import of 1500 records requires the `--i-understand-quality-
  impact` flag and writes an audit record.

**Gates.**

- All M1–M5 gates remain green.
- M6 acceptance criteria covered by tests.
- Quarantine isolation tests pass (canonical index unaffected by quarantine writes).
- Importer-specific scenarios cover the failure modes (malformed markdown, missing
  sections, encoding issues, duplicate detection).

---

## v1.0 release criteria

All of:

- M1 through M6 shipped and green.
- Full validation gate from `docs/decisions/0016-build-approach.md` passes on every PR.
- Conflict-and-merge test suite covers every documented edge case from the stress-test
  scenarios (see ADRs 0003, 0009, 0014, 0017, 0018).
- AGENTS.md, CLAUDE.md, ARCHITECTURE.md, and all component specs in
  `docs/components/` are consistent with the implementation.
- Performance gate: scenario suite under 5 minutes; full validation under 10 minutes.
- Distribution: prebuilt binaries for macOS (arm64, x86_64), Linux (x86_64, arm64),
  Windows (x86_64). Optional cargo install path.
- One real team has used Firetrail for at least two weeks against a production repo
  and reported no critical issues.

---

## What is explicitly not in v1.0

These are deferred. Each represents real value but pulls the v1.0 critical path off
schedule.

- **Auto-sync atomicity policy for external mode.** Bot-mediated branch parity between
  code and data repos. Useful, infrastructure-heavy. Defer to v1.1.
- **Web UI.** CLI is the product (NFR-029). A read-only web viewer may come later.
- **Real-time multi-agent coordination.** Advanced claim-conflict resolution, presence,
  collaborative editing. Out of scope.
- **Bidirectional sync with Jira/GitHub.** Import is supported; pushing changes back
  is deferred.
- **Cross-organization sharing of memory.** Multi-org corpora with access control.
  Out of scope.
- **Hosted Firetrail instance.** v1.0 is local-and-Git only.

---

## Epic mapping

Each milestone decomposes into 5–10 epics. Epics live in beads with `--type=epic`.
Tasks within epics carry `--parent <epic-id>`. The first decomposition pass (after this
roadmap is reviewed) creates the M1 epics with full acceptance criteria. Later
milestones decompose as we approach them.

The next action after this document is to create the M1 epics in beads.

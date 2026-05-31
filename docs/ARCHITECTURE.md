# Firetrail Architecture

This document is the integration view: how the pieces fit together, what each
layer is responsible for, and what flows look like end-to-end.

For decisions and rationale, see `docs/decisions/`. For per-crate specs, see
`docs/components/`. For the strategic plan, see `docs/ROADMAP.md`.

---

## What Firetrail is

A repo-native work graph and incident memory system. Tasks, incidents, findings,
runbooks, decisions, and memory records live as JSON files committed to Git.
SQLite plus `sqlite-vec` is a derived read index. Engineers and AI coding agents
read structured context via `firetrail prime`, write records via the CLI, and
review changes through normal PR review.

Firetrail itself never calls an LLM at runtime. The reasoning layer is the host
agent (Claude Code, Cursor, or a human). Firetrail provides structured context
and structural guardrails. See ADR-0005.

---

## The two-substrate model

Firetrail keeps two kinds of state, with clear ownership:

```
                  ┌───────────────────────────────────┐
                  │              Git                  │  authoritative
                  │  .firetrail/records/<type>/*.json │  source of truth
                  └───────────────────────────────────┘
                                     │
                              (rebuildable from)
                                     ▼
                  ┌───────────────────────────────────┐
                  │            SQLite                 │  derived cache
                  │   .firetrail/index.db             │  rebuildable
                  │     • records table               │  gitignored
                  │     • relations table             │
                  │     • acceptance_criteria         │
                  │     • evidence                    │
                  │     • embeddings_canonical (M3)   │
                  │     • embeddings_quarantine (M6)  │
                  └───────────────────────────────────┘
```

The JSON files are the contract. Anyone can read them with `jq`, edit them in
an editor, diff them in a PR. The SQLite index is a performance cache; if it is
deleted, the next `firetrail` command rebuilds it from the files.

The embedding cache (`~/.cache/firetrail/<repo-hash>/embeddings.db`) is a
*third* derived store, machine-local rather than workspace-local, content-hash
keyed. Switching worktrees does not invalidate it. See ADR-0007.

---

## Crate layering

```
                                  ft-cli
                                    │
        ┌─────┬─────┬─────┬─────┬───┴───┬─────┬─────┬─────┬─────┐
        │     │     │     │     │       │     │     │     │     │
      prime  pr  search trust history index storage  …    git identity
        │     │     │     │     │       │     │           │     │
        └─────┴─────┴──┬──┴─────┴───────┴─────┘           │     │
                       │                                  │     │
                     ft-core ◄────────────────────────────┴─────┘
                       ▲
                       │
                  ft-testkit
```

Direction of arrows: depends on.

`ft-core` carries types every crate consumes. `ft-testkit` is consumed by every
crate's test target. `ft-cli` is the topmost glue layer.

### Crate responsibilities

| Crate | M1 | M2 | M3 | M4 | M5 | M6 | Purpose |
|---|---|---|---|---|---|---|---|
| `ft-core` | ● | ● | ● | ● | ● | ● | Record types, schema, hash chain types |
| `ft-git` | ● | ● | ● | ● | ● | ● | Git operations wrapper |
| `ft-testkit` | ● | ● | ● | ● | ● | ● | Test fixtures, scenario runner |
| `ft-storage` | ● | ● | | | ● | | JSON-in-Git read/write |
| `ft-identity` | ● | | | | ● | | Identity resolution |
| `ft-index` | ● | ● | ● | | | ● | SQLite read index |
| `ft-history` | | ● | | | | | PR-time compaction, hash chain |
| `ft-trust` | | ● | | | | | Trust state machine |
| `ft-embed` | | | ● | | | | ONNX daemon, embedding cache |
| `ft-search` | | | ● | | | ● | Hybrid search + ranking |
| `ft-prime` | | | ● | | | | Context pack generation |
| `ft-pr` | | | | ● | | | PR check, merge driver |
| `ft-scope` | | | | ● | ● | | Multi-scope routing, CODEOWNERS |
| `ft-import` | | | | | | ● | Markdown / Jira / Confluence imports |
| `ft-cli` | ● | ● | ● | ● | ● | ● | CLI entry; extended per milestone |

Filled dots indicate which milestone is the first to add or substantially
extend that crate.

---

## Key invariants

These hold across the codebase and are tested.

### 1. Records are the source of truth

Anywhere a query could be answered from the index or from the files, the files
win. The index is rebuilt when in doubt. `firetrail doctor` reconciles the
index against the file tree on demand.

### 2. Every record write recomputes `state_hash`

A record whose `state_hash` does not match its canonical content is invalid.
`ft-storage` refuses to write such a record and refuses to read one without
flagging `HashMismatch`. See ADR-0017.

### 3. `prev_state_hash` chain spans merged history

Each merge into the long-lived branch updates `prev_state_hash` (via
`ft-history`'s PR-time compaction). Force-pushes that rewrite merged history
break the chain; `firetrail verify` detects breaks. See ADR-0003 and ADR-0017.

### 4. IDs are full content hashes, displayed by adaptive prefix

Stored IDs are full SHA-256 hex. Display uses the shortest unambiguous prefix
within the current view, minimum 6 hex chars. See ADR-0015.

### 5. Memory records require memory-only PRs

`finding`, `decision`, `incident`, `runbook` cannot share a commit with code
changes. Enforced by pre-commit hook and re-checked by `firetrail check pr`.
See ADR-0009.

### 6. Imports land in quarantine

`origin: imported` records go to a separate index table. Default search and
prime exclude them. Promotion is explicit. See ADR-0014.

### 7. Trust transitions require evidence and identity rules

`draft → reviewed` requires evidence. `reviewed → verified` requires a second
human distinct from the PR author. Agents cannot promote `verified` records.
Risk-class records (security, availability, data-loss, compliance) need
verified status before appearing in default prime output. See ADR-0013.

### 8. The tool never calls an LLM

Firetrail produces context; the host agent reasons. Embeddings are a local
ONNX model, not an LLM API. See ADR-0005 and ADR-0007.

### 9. Offline-first

Every core command works without network. Network-dependent commands are
clearly partitioned. See ADR-0011.

### 10. The skill is documentation, not a tier

`.claude/skills/firetrail/SKILL.md` instructs Claude Code on how to drive the
CLI. There is no no-install mode. See ADR-0012.

---

## Storage modes

Two modes selected at `firetrail init`. Both implement the same `Storage` trait
from `ft-storage`.

### Embedded mode (default)

```
my-monorepo/
├── .firetrail/
│   ├── config.yml
│   ├── records/<type>/<id>.json
│   ├── index.db                    (gitignored)
│   └── hooks/
└── …code…
```

Records, code, and Firetrail config all in one repo. Atomic with code. Default
for single-repo and monorepo teams.

### External mode (M5)

```
my-monorepo/
├── .firetrail.toml                 (small config file, ~10 lines)
└── …code…

org/firetrail-data/                  (separate repo)
├── records/<type>/<id>.json
└── identity.yml
```

Records live in a separate "data repo" that one or more code repos point at.
Better for multi-repo orgs. PR-link enforcement keeps cross-repo references
honest. See ADR-0006 and ADR-0010.

---

## Multi-scope routing

Every record carries:

- `owningScope` — single scope that owns review authority.
- `affectedScopes` — additional scopes the record is relevant to.
- `appliesTo` — path globs that govern decision applicability.

Default queries filter by current scope (detected from cwd). Search uses
scope-distance ranking. CODEOWNERS resolves aggregated review requirements.
See ADR-0004.

---

## Identity model

Every actor resolves to a canonical `Identity`. At M1, resolution walks the env
var, local config, git config in order. At M5, an identity registry adds
aliases, kinds (`human` / `bot` / `ci`), capabilities, lifecycle (`active` /
`offboarded`), and on-behalf-of for CI runners.

Records carry `created_by`, `claimed_by`, `reviewer`, `checked_by`. Claims
require `claim_expires_at` to prevent zombies. Offboarded identities cannot
hold live claims (sweep job releases them automatically). See ADR-0008.

---

## Trust state machine

```
                      evidence + review
        draft ────────────────────────────► reviewed
          │                                    │
          │                                    │ second reviewer
          │                                    ▼
          │                                 verified
          │                                    │
          │ idle 14d                           │ disagreement
          ▼                                    ▼
        stale                              deprecated
          │                                    │
          ▼                                    │
        archived ◄──────────────────────────── ┘
```

`origin: agent | human | imported` flag persists across transitions.
`risk_class` (security, availability, data-loss, compliance) tightens
requirements. See ADR-0013.

---

## Repo profile

`RepoProfile` is a record kind (`RecordKind::RepoProfile`, prefix `PROFILE`,
stored under `records/repo_profile/`) that holds a small, always-read set of
facts about the host repo: the canonical validate command, the standard
test/build/lint commands, language/tooling facts (`languages`,
`package_managers`, `runtime`), and a shallow component map (names + paths only,
via `ComponentRef`). It is the foundation later subsystems read from —
architecture docs, repo rules, and the audit loop all need these facts.

**Singleton, by convention.** One `RepoProfile` per repo. `ft-storage` exposes
`profile_get` / `profile_set` helpers (in `ft-storage/src/profile.rs`) that read
and upsert the singleton through the `Storage` trait — `profile_set` updates the
existing record in place if present, else creates it.

**Where it lives.** In external storage mode the profile is written to the
separate data repo (cloned under `.firetrail/cache/data-repo/`, gitignored in the
host), keeping the host repo clean as artifacts accumulate. In embedded mode it
lives alongside the other records.

**Trust lifecycle is the propose→confirm signal.** This is a direct application
of ADR-0005: the *agent decides, firetrail stores*. The agent inspects the repo,
discusses with the user, and writes the profile as `Draft` (`origin: agent`) —
its proposal. A human confirming transitions it `Draft → Reviewed → Verified`
through the existing trust machine (no bespoke review path). Firetrail ships no
language/tooling auto-detection in Rust; that judgment lives in the
`firetrail-bootstrap` skill.

**Surfaces.** The profile is written/read through:

- `firetrail profile show | set | component add | rm` — the CLI the bootstrap
  skill drives (partial-update semantics).
- `firetrail doctor` — warns when the profile is missing or `validate_command` is
  empty, info when it is still `Draft`; `--strict` exits non-zero for CI
  enforcement.
- `/api/profile` (+ `/api/profile/components`) in ft-ui — a read/edit Profile
  panel; confirmation goes through the existing `/api/trust/*` routes.
- The `firetrail-bootstrap` skill — the agent-run conversation that populates it.

See `docs/components/repo-profile.md` and the design spec
`docs/specs/2026-05-31-repo-profile-bootstrap-design.md` (epic `firetrail-lj41`).
This is sub-project A; architecture docs, repo rules, and the audit loop that
build on it are future work tracked separately.

---

## Flows

Selected flows that touch multiple crates.

### Create a task

```
firetrail task create "Add Redis pool alert" --epic <id>

ft-cli
  └─ parse args, resolve identity (ft-identity)
     └─ build Record (ft-core; RecordId minted, state_hash computed)
        └─ Storage::write (ft-storage)
           ├─ write JSON atomically under .firetrail/records/task/
           └─ Index::refresh (ft-index)
              └─ upsert row in records table; insert deps from --epic
        └─ render result (markdown or JSON)
```

### Ready detection

```
firetrail ready

ft-cli
  └─ parse filters
     └─ Index::ready (ft-index)
        └─ SQL query (records LEFT JOIN relations) filtering out blocked + claimed
           └─ return list of IndexedRecord
              └─ render board-style or list output
```

### Search (M3)

```
firetrail search "redis pool latency"

ft-cli
  └─ Search::query (ft-search)
     ├─ embed query via ft-embed (daemon)
     ├─ sqlite-vec ANN query against canonical embeddings table
     ├─ optional BM25 lexical pass
     └─ hybrid rank: similarity × trust × recency × scope-distance
        └─ render results
```

### Prime (M3)

```
firetrail prime --task <id> --max-tokens 8000

ft-cli
  └─ Prime::for_task (ft-prime)
     ├─ load the record (ft-storage)
     ├─ load acceptance criteria + evidence (ft-storage)
     ├─ walk direct relations (ft-index)
     ├─ vector neighbors filtered by scope and trust (ft-search)
     ├─ priority-ordered context assembly under token budget
     └─ render markdown or JSON with omitted manifest
```

### PR check (M4)

```
firetrail check pr  (in CI on every PR)

ft-cli
  └─ Check::run_pr (ft-pr)
     ├─ enumerate changed records via ft-git::diff
     ├─ for each record:
     │   ├─ validate hash chain (ft-history)
     │   ├─ validate trust transitions (ft-trust)
     │   ├─ scope authorization via CODEOWNERS (ft-scope)
     │   ├─ memory-only-PR enforcement (cross-reference against code files)
     │   ├─ acceptance-criteria completeness on closing tasks
     │   ├─ evidence presence on reviewed/verified transitions
     │   └─ secret scan + AC cap + draft-age checks
     └─ summary report (markdown comment + exit code)
```

---

## Test harness layers

Inner-loop development depends on Layers 0–2. CI runs all five. See ADR-0016
for the full rationale.

| Layer | What | Tool | Target time |
|---|---|---|---|
| 0 | Compile | `cargo check`, `cargo build` | < 5 s per crate |
| 1 | Unit | `cargo nextest run -p <crate>` | < 1 s per crate |
| 2 | Property | `proptest` over factories | seconds |
| 3 | Integration | tests against `TestRepo` + real SQLite + real git | < 2 min workspace |
| 4 | Scenarios | YAML scenarios against the binary | < 5 min workspace |
| 5 | Conflict / merge | two-branch + force-push + squash drills | < 5 min workspace |

Full validation under 10 minutes; pre-commit hook runs a fast subset
(fmt + clippy + Layer 0/1) on staged files.

---

## How parallel subagent work fits

Crates form a dependency DAG. Implementation proceeds in waves. Within a wave,
crates are independent and can be built by separate subagents in parallel git
worktrees.

```
Wave 1 (foundation):      ft-core, ft-git, ft-testkit
Wave 2 (parallel):        ft-storage, ft-identity, ft-history
Wave 3 (parallel):        ft-index, ft-embed, ft-scope, ft-trust
Wave 4 (parallel):        ft-search, ft-prime, ft-import, ft-pr
Wave 5:                   ft-cli
```

Each subagent receives the component spec, the relevant ADRs, the crate
skeleton, and a list of required tests. A second subagent runs as an
independent verifier: writes additional tests, runs them, reports results.
See ADR-0016 and `AGENTS.md`.

---

## Where to go next

- **For decisions and why:** `docs/decisions/0001-rust-over-go.md` and the
  numbered ADRs that follow.
- **For per-crate implementation contracts:** `docs/components/<crate>.md`.
- **For the milestone plan:** `docs/ROADMAP.md`.
- **For the build conventions and validation gates:** `AGENTS.md`.
- **For the original product brief (historical):** `requirements.md`.

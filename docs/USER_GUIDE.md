---
doc_type: reference
status: reviewed
scope: ft-cli
---

# Firetrail User Guide

Firetrail is a **repo-native work graph and incident memory system**. Your
tasks, dependencies, findings, runbooks, and decisions are stored as JSON
inside your git repository, indexed locally for search, and *primed* into an
AI agent's context on demand.

This is the end-to-end guide: install, the mental model, every workflow, the
web UI, semantic search, and a command reference. For the system design see
[`ARCHITECTURE.md`](ARCHITECTURE.md); for milestones see
[`ROADMAP.md`](ROADMAP.md); for the rationale behind specific choices see the
numbered ADRs under [`decisions/`](decisions/).

---

## Table of contents

- [1. Mental model](#1-mental-model)
- [2. Install & build](#2-install--build)
- [3. Initialise a workspace](#3-initialise-a-workspace)
- [4. The work graph](#4-the-work-graph)
- [5. Acceptance criteria & lifecycle](#5-acceptance-criteria--lifecycle)
- [6. Relations & dependencies](#6-relations--dependencies)
- [7. Incident memory](#7-incident-memory)
- [8. Docs linked to work](#8-docs-linked-to-work)
- [9. Search](#9-search)
- [10. Prime: build an agent context pack](#10-prime-build-an-agent-context-pack)
- [11. Semantic embeddings & the model](#11-semantic-embeddings--the-model)
- [12. The embedding daemon](#12-the-embedding-daemon)
- [13. The web UI](#13-the-web-ui)
- [14. PR safety & history integrity](#14-pr-safety--history-integrity)
- [15. Multi-scope & identity](#15-multi-scope--identity)
- [16. Importing historical markdown](#16-importing-historical-markdown)
- [17. Repo profile & doctor](#17-repo-profile--doctor)
- [18. Output formats & scripting](#18-output-formats--scripting)
- [19. Workspace layout & configuration](#19-workspace-layout--configuration)
- [20. Environment variables](#20-environment-variables)
- [21. Troubleshooting](#21-troubleshooting)
- [22. Command reference](#22-command-reference)

---

## 1. Mental model

Firetrail has **two substrates** (ADR-0002, ADR-0006):

- **Git is the source of truth.** Every record is a JSON file under
  `.firetrail/records/<kind>/`. Records are versioned, diffable, and
  PR-reviewable like any other code. There is no external database to keep in
  sync — your repo *is* the database.
- **SQLite is a derived cache.** `.firetrail/index.db` holds the SQL index and
  the FTS5 + vector search tables. It is rebuildable at any time from the JSON
  (`firetrail index rebuild`) and is git-ignored.

Records fall into three families:

| Family | Kinds | Purpose |
|---|---|---|
| **Work graph** | `epic`, `task`, `subtask`, `bug` | What to do, who owns it, what blocks it |
| **Memory** | `incident`, `finding`, `runbook`, `decision`, `gotcha`, `memory` | What you learned, so the next person (or agent) doesn't relearn it |
| **Docs** | `doc` | A thin pointer to a markdown file, linked to work items |

Everything is **offline-first** (ADR-0011): no network calls are made unless
you explicitly ask for them (e.g. `--download-model`, `import refresh`).

---

## 2. Install & build

Firetrail is a Rust workspace. You need a recent stable toolchain (see
`rust-toolchain.toml`).

### Build the CLI

```sh
cargo build --release -p ft-cli --bin firetrail
# Binary lands at target/release/firetrail
```

**Semantic search is on by default.** The embedder uses
[`tract`](https://github.com/sonos/tract) — a **pure-Rust** ONNX inference
engine — so the default build links **no native ONNX runtime** and stays fully
portable. The `onnx` feature of `ft-embed` is enabled by default and is pulled
in transitively by both `ft-cli` and `ft-ui`.

If you want a smaller, embeddings-free build that only does lexical (FTS5)
search and uses the deterministic mock embedder, opt out of default features:

```sh
cargo build --release -p ft-cli --bin firetrail --no-default-features
```

> **Note:** building with the `onnx` engine compiled in is not the same as
> having a model on disk. The first time you run a semantic search, firetrail
> needs the `bge-small-en-v1.5` model files locally — see
> [§11](#11-semantic-embeddings--the-model). Without them it transparently
> falls back to the mock embedder; lexical search always works.

### Build the web UI

The UI is a Vite/React SPA embedded into the `ft-ui` server binary via the
`bundled-ui` feature. The repo ships a [`justfile`](../justfile) with the
recipes:

```sh
just ui-build     # pnpm install + pnpm build + cargo build -p ft-ui --features bundled-ui --release
just ui           # build, then run the production server
just ui-dev       # Vite (:5173) + ft-ui (:5174) with hot reload
```

Or run the steps directly:

```sh
pnpm -C crates/ft-ui/web install
pnpm -C crates/ft-ui/web build
cargo build --release -p ft-ui --features bundled-ui
```

The `bundled-ui` feature embeds `crates/ft-ui/web/dist/` into the binary at
**compile time**. If you rebuild the web bundle, force a re-link of `ft-ui`
(e.g. `touch crates/ft-ui/src/assets.rs`) so the fresh assets are baked in —
cargo does not track the `dist/` directory for changes on its own.

> **pnpm / corepack gotcha:** `pnpm -C crates/ft-ui/web …` resolves the package
> manager from the *current* directory, walking up the tree. If a parent
> directory (e.g. your home folder) has a `package.json` declaring
> `packageManager: yarn`, corepack will refuse to run pnpm. Run pnpm from
> inside `crates/ft-ui/web` (where `packageManager: pnpm@…` is declared) to
> avoid this.

### Updating firetrail

Update an installed binary to the latest release:

    firetrail upgrade            # install the latest release
    firetrail upgrade --check    # report whether a newer release exists

`upgrade` only works for binaries installed via the Firetrail installer
(the `curl … | sh` script from a GitHub release), which records an install
receipt. For `cargo install` or hand-copied builds it prints how to update
instead. Note: `firetrail update <id>` is unrelated — it edits a record.

---

## 3. Initialise a workspace

Run `init` inside an existing git repo:

```sh
firetrail init                 # interactive on a TTY; prompts for the choices below
firetrail init --non-interactive   # accept defaults, good for scripts
firetrail doctor               # verify the workspace is healthy
```

`init` creates `.firetrail/` (records + config + index), writes an `AGENTS.md`
and a `.claude/skills/firetrail/` skill so AI agents know how to use the tool,
and installs git hooks. Useful flags:

| Flag | Effect |
|---|---|
| `--download-model` | Fetch the `bge-small-en-v1.5` model (~33 MiB) into the machine-local cache now. Off by default (offline-first). |
| `--no-agents` | Skip writing `AGENTS.md` / `.claude/skills/firetrail/`. |
| `--no-hooks` | Skip installing git hooks. |
| `--strict-identity` | Reject identities not present in the registry (persists to `config.yml`). |
| `--storage-mode embedded\|external` | Where records live. `embedded` (default) keeps them in this repo; `external` (M5) uses a separate data repo. |
| `--pilot <ids>` | Enable only this comma-separated list of scopes in `scopes.yaml`. |

After `init`, the workspace is discovered automatically from your current
directory. Override it anywhere with `--workspace <path>`.

---

## 4. The work graph

Work-graph records have a positional `<TITLE>` and optional flags. **Epics,
tasks, subtasks, and bugs are created via a `create` subcommand:**

```sh
firetrail epic create "Ship v1" --priority p1
firetrail task create "Build auth" --epic <epic-id> --priority p2
firetrail subtask create "Add password reset" --parent <task-id>
firetrail bug create "Login 500 on empty email" --service auth --severity high
```

Common create flags: `--description`, `--priority {p0..p4}`, `--scope`,
`--owner` (task/subtask), `--epic` (task), `--parent` (subtask, required),
`--service`/`--severity` (bug), and repeatable `--label key=value`.

> Priorities are `p0`–`p4` (`p0` = critical/top-of-queue, `p4` = backlog).

### Find and view work

```sh
firetrail ready                       # unblocked work, optionally --type/--owner/--scope/--limit
firetrail board                       # kanban-style status board
firetrail list --status in_progress   # filter by --type/--status/--owner/--scope, paginate with --limit/--offset
firetrail show <id>                   # full envelope, body, and relations
firetrail graph                       # ASCII dependency tree
```

### Update, claim, close

```sh
firetrail update <id> --status in_progress --priority p1 --owner alice
firetrail claim <id>                  # mint a Claim (default 7d; override with --expires 12h)
firetrail unclaim <id>                # release your active claim
firetrail close <id>                  # validates acceptance criteria first
firetrail reopen <id>
```

Statuses: `open`, `ready`, `in_progress`, `review`, `blocked`, `closed`,
`deferred`, `archived`.

---

## 5. Acceptance criteria & lifecycle

A record cannot be `close`d until its acceptance criteria are checked (unless
you force it).

```sh
firetrail criteria add <id> "Login works with valid creds"
firetrail criteria add <id> "Lockout after 5 failures"
firetrail criteria list <id>
firetrail criteria check <id> 1            # 1-based index (or the `ac-NN` id)
firetrail criteria uncheck <id> 1
firetrail criteria evidence <id> 1 <url>   # attach proof

firetrail close <id>                 # fails if any criterion is unchecked
firetrail close <id> --force --reason "descoped; tracked in firetrail-xyz"
```

`--reason` is **required** when you `--force` a close, so the override is always
auditable.

---

## 6. Relations & dependencies

`link` creates a typed relation between two records; `dep` is a shortcut for
dependency edges.

```sh
firetrail link <from> <to> --type blocks
firetrail dep add <blocked> <blocker>      # convenience for blocks/blocked-by
firetrail dep remove <a> <b>
```

Relation kinds: `blocks`, `blocked-by`, `parent-of`, `child-of`, `related-to`,
`duplicates`, `supersedes`, `fixed-by`, `caused-by`.

> Parent/child edges between epics⇄tasks and tasks⇄subtasks are derived
> automatically from the `--epic` / `--parent` flags at create time; you only
> need `link`/`dep` for the other relation types.

---

## 7. Incident memory

Memory records capture what you learned. Each kind has a dedicated create
command; there is also a generic `memory create` and a quick `capture`.

```sh
firetrail incident create "Checkout 500s on Tuesday"
firetrail finding create "Redis OOM under spike"
firetrail gotcha create "tract requires inputs bound by name, not position"
firetrail runbook create "Restart the embedding daemon" --summary "Steps to bounce the daemon"
firetrail decision create "Adopt tract over ort" --context "ort needs a native runtime" --decision "Use pure-Rust tract"

# Quick opportunistic capture (pick the kind inline):
firetrail capture --title "Flaky test in ft-ui" --kind gotcha --body "…" --tags e2e,flaky
```

Each memory kind is created via its `create` subcommand. `incident`, `finding`,
and `gotcha` take a positional `<SUMMARY>`; `runbook` takes a `<TITLE>` plus
`--summary`; `decision` takes a `<TITLE>` plus `--context` and `--decision`.
Run `firetrail <kind> create --help` for the full field set. `runbook` also has
a `runbook step` subcommand for managing ordered steps.

### Memory lifecycle & trust

Memory records carry a **trust state** and move through a lifecycle. Higher
trust ranks higher in search and `prime`:

```
draft  →  reviewed  →  verified
                    ↘  deprecated  →  archived (terminal)
```

```sh
firetrail memory list                 # also: --stale to surface aging records
firetrail memory show <id>
firetrail memory review <id>          # draft → reviewed
firetrail memory promote <id>         # reviewed → verified
firetrail memory deprecate <id>
firetrail memory archive <id>         # terminal
firetrail memory supersede <old> --with <new>
firetrail memory merge <ids…>         # collapse duplicates into a canonical record
firetrail memory redact <id>          # irreversible body wipe (keeps the record shell)
```

`firetrail memory salvage` rescues memory records from an abandoned or
about-to-be-deleted branch (ADR-0018).

---

## 8. Docs linked to work

A `doc` record is a **thin pointer** to a markdown file — the file stays the
single source of truth; firetrail stores the path, a `content_hash` for drift
detection, a summary, and relations to work items. Linked docs are surfaced by
`prime`. See [`DOCS.md`](DOCS.md) for the full convention.

```sh
firetrail doc add docs/specs/auth-design.md     # adopt an existing markdown file
firetrail doc link <doc-id> <task-or-epic-id>   # creates a documented-in relation
firetrail doc index                             # refresh content_hash + summary + search
```

You can also add a YAML frontmatter block (`doc_type`, `status`, `links:`,
`scope:`) to any markdown file and `firetrail doc index` will read it verbatim.

---

## 9. Search

```sh
firetrail search "redis oom"                    # hybrid by default (auto)
firetrail search "redis oom" --mode lexical     # FTS5 only — always available
firetrail search "memory pressure" --mode vector
firetrail search "auth" --kind finding,task --trust verified --scope ft-auth --limit 20
firetrail similar <record-id>                   # nearest records to one you already have
```

Search modes (`--mode`):

| Mode | Behaviour |
|---|---|
| `auto` (default) | Engine picks the best signal mix. |
| `lexical` | FTS5 lexical only. Works without any model. |
| `hybrid` | BM25 lexical + 384-dim vector, trust/recency-weighted. |
| `vector` | Vector similarity only. |

Other flags: `--trust <state>`, `--kind <k1,k2>`, `--scope`, `--limit`,
`--embedder <name>` (force a specific embedder), `--include-quarantine`.

---

## 10. Prime: build an agent context pack

`prime` is the heart of firetrail's agent integration. It assembles a
token-budgeted context pack for a task or a free-text query — the linked docs,
relevant memory, and surrounding work — so an agent picking up a ticket starts
with the right, current context (ADR-0019).

```sh
firetrail prime --task <task-id>                 # pack for a specific ticket
firetrail prime --query "how do we handle auth retries"
firetrail prime --task <id> --max-tokens 8000 --min-trust reviewed --kind finding,decision
```

Linked docs are delivered as **link + summary + path** (not inlined), so a
2,000-line architecture doc never blows the budget — the agent reads the full
file on demand. Output is markdown on a TTY, JSON otherwise (or force with
`--json`).

---

## 11. Semantic embeddings & the model

Real semantic search uses the **`bge-small-en-v1.5`** model (int8-quantized,
~33 MiB) run through pure-Rust `tract`. Two things are independent:

1. **The engine** — compiled in by default (the `onnx` feature). A
   `--no-default-features` build omits it entirely.
2. **The model files** — `model.onnx` + `tokenizer.json`, fetched on demand.

Get the model on disk:

```sh
firetrail init --download-model     # download during init
# …or any time later:
firetrail migrate                   # re-embed/re-index after a model becomes available
```

**Resolution & caching:**

- Default model dir: `<cache_home>/firetrail/models/bge-small-en-v1.5/`, where
  `<cache_home>` is `$FIRETRAIL_CACHE_HOME` if set, else `$HOME/.cache`.
- Override the model location with `FIRETRAIL_BGE_MODEL_DIR`.
- Embeddings are cached per-repo at
  `<cache_home>/firetrail/<repo-hash>/embeddings.db`, so multiple worktrees of
  the same repo share one cache.

**Fallback:** if the engine is compiled in but no model is on disk, firetrail
prints a warning and falls back to the deterministic **mock embedder**. Lexical
search keeps working fully either way. The embedder is configured in
`.firetrail/config.yml` under `embeddings:` (`provider: local | mock | lexical`,
`fallback: mock | lexical | none`).

---

## 12. The embedding daemon

Embedding is CPU work; the daemon keeps the model warm so searches stay fast
(ADR-0007). It is Unix-only and optional — searches work without it, just
colder.

```sh
firetrail daemon start                         # background; --foreground to stay attached
firetrail daemon status
firetrail daemon stop
firetrail daemon start --idle-timeout-secs 600 # auto-exit after idle
```

---

## 13. The web UI

Firetrail ships a local web UI — a human-facing mirror of what `prime` delivers
to agents (board, tickets, memory, search, docs, identity, trust, audit).

```sh
firetrail ui                          # starts the server and opens a browser
firetrail ui --port 5174 --no-open
```

`firetrail ui` requires a binary built with the `bundled-ui` feature (or run
`ft-ui --dev` alongside Vite). The server is **auth-gated**: on start it prints
a one-time bootstrap URL containing a `?token=…`. Open that URL to establish a
signed session cookie; subsequent requests without the cookie get `401`.

For development, `just ui-dev` runs Vite on `:5173` and the server on `:5174`
with `--dev` (which relaxes the `Origin` check so Vite can talk to the API).

The TypeScript wire types are generated from the Rust `ft-ops` types — keep
them in sync with `just ui-gen-ts` / verify with `just ui-check-ts`.

---

## 14. PR safety & history integrity

Records are JSON in git, so they go through PR review like code. Firetrail adds
guardrails so merges and history stay consistent (ADR-0003, ADR-0010,
ADR-0017).

```sh
firetrail merge-driver-install        # install the JSON merge driver into this repo
firetrail check pr                    # validate records changed between two git refs
firetrail check paths <paths…>        # per-commit path validation (the pre-commit hook surface)
firetrail lint memory --fix           # lint workspace state; --fix applies safe remediations
firetrail verify                      # per-record history-chain integrity
firetrail compact                     # PR-time history compaction
firetrail diff <ref-a> <ref-b>        # record-aware diff between two git refs
firetrail server-hooks <dest>         # install server-side hook templates
```

The merge driver resolves record conflicts field-aware instead of line-aware, so
two people editing different fields of the same record don't collide.

---

## 15. Multi-scope & identity

Large repos partition work into **scopes** (e.g. `ft-ui`, `ft-core`) declared
in `.firetrail/scopes.yaml`. Scope resolution is **last-declared-wins**, so
declare broad scopes first (ADR-0004).

```sh
firetrail scope list
firetrail scope show <id>
firetrail scope add <id> --applies-to "crates/ft-ui/**" # appended last (also: --name --alias --codeowners)
firetrail scope owners crates/ft-ui/src/server.rs      # resolve CODEOWNERS for a path
firetrail scope reorder <id1> <id2> …                  # order is semantic
```

Identities are a registry of actors (ADR-0008):

```sh
firetrail identity register --id alice --email alice@example.com
firetrail identity list
firetrail identity offboard <id>
firetrail claim-takeover <task-id>    # admins only — take over a live claim
```

With `identity.strict: true` (set via `init --strict-identity`), only
registered identities may act.

---

## 16. Importing historical markdown

Firetrail does **not** integrate with Jira/Confluence directly (ADR-0014
addendum) — instead, your agent uses its own MCP servers and pipes markdown in.
Imports land in a **quarantine** and must be promoted before they join the
canonical corpus.

```sh
firetrail import incidents docs/incidents/
firetrail import adrs docs/decisions/
firetrail import runbooks docs/runbooks/
firetrail import refresh                  # re-pull/re-parse already-imported records

firetrail promote-import --interactive    # review and promote quarantined records
```

---

## 17. Repo profile & doctor

The **repo profile** is a singleton bag of repo facts — build/test/validate
commands, tooling, a shallow component map — that agents maintain and `doctor`
checks.

```sh
firetrail profile show
firetrail profile set --validate "just ci"
firetrail profile component add <name> "crates/ft-ui/**"
firetrail profile resolve <changeset>     # which validate commands a change requires
firetrail doctor                          # health check
firetrail doctor --fix                    # rebuild index, reinstall hooks
firetrail doctor --strict                 # CI: non-zero exit if profile missing/unverified
```

---

## 18. Output formats & scripting

Every command auto-detects its output: **markdown on a TTY, JSON otherwise**.
Force it explicitly:

```sh
firetrail list --json                 # or --format json
firetrail show <id> --format markdown
firetrail ready --json | jq '.[].id'
```

Global flags work on every command: `--json` / `--format`, `-q/--quiet`,
`-v/--verbose` (tracing to stderr), `--workspace <path>`.

---

## 19. Workspace layout & configuration

`firetrail init` creates:

```
.firetrail/
├── config.yml          # workspace config (format_version, storage, identity, claim, embeddings)
├── identity.yml        # local actor identity
├── index.db            # derived SQLite index (git-ignored, rebuildable)
├── cache/              # local scratch
└── records/            # the source of truth — one dir per kind
    ├── epic/  task/  subtask/  bug/
    ├── incident/  finding/  runbook/  decision/  gotcha/  memory/
    ├── doc/
    └── repo_profile/
AGENTS.md               # how agents should use firetrail (skip with --no-agents)
.claude/skills/firetrail/
```

`config.yml` highlights:

```yaml
format_version: 1
storage:
  mode: embedded
identity:
  strict: false
claim:
  default_duration: 7d
embeddings:
  provider: local      # local | mock | lexical
  model: bge-small-en-v1.5
  fallback: mock       # mock | lexical | none
```

---

## 20. Environment variables

| Variable | Purpose |
|---|---|
| `FIRETRAIL_CACHE_HOME` | Override the cache root (default `$HOME/.cache`). Models live under `<cache_home>/firetrail/models/`, embeddings under `<cache_home>/firetrail/<repo-hash>/`. |
| `FIRETRAIL_BGE_MODEL_DIR` | Point directly at a directory containing `model.onnx` + `tokenizer.json`. |
| `HOME` | Required to resolve the default cache root when `FIRETRAIL_CACHE_HOME` is unset. |

---

## 21. Troubleshooting

| Symptom | Cause / fix |
|---|---|
| `not an initialised firetrail workspace` | Run `firetrail init` in the repo, or pass `--workspace <path>`. |
| Semantic search returns mock-looking / lexical-only results | The model isn't on disk. Run `firetrail init --download-model` (or set `FIRETRAIL_BGE_MODEL_DIR`), then `firetrail migrate`. |
| Search results look stale after editing records by hand | Rebuild the index: `firetrail index rebuild` (or `index refresh`). |
| `firetrail ui` / `ft-ui` serves a "not bundled" placeholder | The binary was built without `--features bundled-ui`, or `web/dist/` was rebuilt without re-linking. Re-run `just ui-build`. |
| Web UI returns `401 unauthorized` | Open the printed `?token=…` bootstrap URL to set the session cookie. |
| `pnpm` refuses with "configured to use yarn" | A parent `package.json` declares yarn; run pnpm from inside `crates/ft-ui/web`. |
| `close` fails with unmet criteria | Check the criteria (`criteria check`), or `close --force --reason "…"`. |
| `doctor --strict` exits non-zero in CI | The repo profile is missing/unverified — `firetrail profile set --validate "…"` and verify it. |

When in doubt, `firetrail doctor` reports actionable issues and `--fix` applies
the safe ones.

---

## 22. Command reference

Run `firetrail <command> --help` for full flags. Top-level commands:

**Workspace:** `init`, `doctor`, `migrate`, `index {rebuild,refresh}`

**Work graph:** `epic create`, `task create`, `subtask create`, `bug create`,
`update`, `close`, `reopen`, `claim`, `unclaim`, `claim-takeover`,
`criteria {add,list,check,uncheck,evidence}`, `link`, `dep {add,remove}`,
`show`, `list`, `ready`, `board`, `graph`

**Memory:** `incident`, `finding`, `runbook`, `decision`, `gotcha`, `capture`,
`memory {create,list,stale,show,review,promote,deprecate,archive,supersede,merge,redact,salvage}`

**Docs:** `doc {add,link,index}`

**Search & prime:** `search`, `similar`, `prime`, `daemon {start,stop,status}`

**Web UI:** `ui`

**PR safety:** `merge-driver-install`, `server-hooks`, `check {pr,paths}`,
`lint`, `review`, `verify`, `compact`, `diff`

**Multi-scope & identity:** `identity {register,list,show,offboard}`,
`scope {list,show,aliases,owners,add,edit,rm,reorder}`, `sync`

**Import:** `import {incidents,adrs,runbooks,refresh}`, `promote-import`

**Profile:** `profile {show,set,list,resolve,component}`

---

*Found a gap or an inaccuracy? This guide is itself a firetrail doc — edit
`docs/USER_GUIDE.md` and run `firetrail doc index`.*

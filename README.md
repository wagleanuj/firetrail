# Firetrail

A repo-native work graph and incident memory system. Tasks, dependencies,
findings, runbooks, and decisions stored as JSON in your git repository,
searchable locally, primed into AI agent context.

📖 **[User Guide](docs/USER_GUIDE.md)** — the complete walkthrough (install,
every workflow, the web UI, semantic search, command reference). The sections
below are excerpts. See also [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) for
the design and [`docs/ROADMAP.md`](docs/ROADMAP.md) for the v1.0 milestones.

## Status

Local work graph (M1), incident memory (M2), search + prime (M3), PR safety
(M4), multi-scope + identity (M5), and markdown importers (M6) are
implemented. Deferred per ADR: strict/auto-sync external storage,
evidence URL fetching. Jira/Confluence integration is out of scope by
design (ADR-0014 addendum) — the calling agent uses its own MCP servers
and pipes markdown into `firetrail import …`.

## Build

```sh
cargo build --release -p ft-cli --bin firetrail
# Binary lands at target/release/firetrail
```

Real semantic search is **on by default**: the embedder runs `bge-small-en-v1.5`
through the pure-Rust [`tract`](https://github.com/sonos/tract) engine, so the
default build links **no native ONNX runtime**. For a smaller, embeddings-free
build (lexical FTS5 search + deterministic mock embedder):

```sh
cargo build --release -p ft-cli --bin firetrail --no-default-features
```

Having the engine compiled in is separate from having the model on disk —
fetch it with `firetrail init --download-model`. Until it's present, firetrail
falls back to the mock embedder; lexical search always works. See
[User Guide §11](docs/USER_GUIDE.md#11-semantic-embeddings--the-model).

To build and run the web UI:

```sh
just ui-build     # pnpm build + cargo build -p ft-ui --features bundled-ui --release
just ui           # build, then run the production server
just ui-dev       # Vite (:5173) + ft-ui (:5174) with hot reload
```

## Quick start

```sh
# 1. Scaffold a workspace inside an existing git repo
firetrail init
firetrail doctor

# 2. Track work  (epics/tasks/subtasks/bugs take a positional TITLE + a `create` subcommand)
firetrail epic create "Ship v1"
firetrail task create "Build auth" --epic <epic-id>
firetrail criteria add <task-id> "Login works"
firetrail ready                       # unblocked work
firetrail claim <task-id>
firetrail board
firetrail close <task-id>             # validates acceptance criteria

# 3. Capture memory  (each kind has a `create` subcommand)
firetrail finding create "Redis OOM under spike"
firetrail incident create "Checkout 500s on Tuesday"
firetrail verify                      # history-chain integrity

# 4. Search and prime an agent
firetrail daemon start                # keep the embedding model warm (Unix only)
firetrail search "redis"
firetrail prime --task <task-id>      # build a context pack
firetrail daemon stop

# 5. PR safety
firetrail merge-driver-install
firetrail check pr
firetrail lint memory --fix           # remediation hints

# 6. Multi-scope, identities
firetrail identity register --id alice --email alice@example.com
firetrail scope list
firetrail claim-takeover <task-id>    # admins only on live claims

# 7. Import historical markdown
firetrail import incidents docs/incidents/
firetrail promote-import --interactive

# 8. Browse it all in the web UI
firetrail ui
```

Run `firetrail --help` for the full command surface, or see the
[command reference](docs/USER_GUIDE.md#22-command-reference).

## Working in this repo

```sh
just            # build + test + clippy + fmt-check
just test       # tests via cargo-nextest if present
just lint       # clippy -D warnings
```

Issue tracking lives in `bd` (beads). Run `bd ready` to see open work.

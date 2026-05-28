# Firetrail

A repo-native work graph and incident memory system. Tasks, dependencies,
findings, runbooks, and decisions stored as JSON in your git repository,
searchable locally, primed into AI agent context.

See `docs/ARCHITECTURE.md` for the design and `docs/ROADMAP.md` for the v1.0
milestones.

## Status

Local work graph (M1), incident memory (M2), search + prime (M3), PR safety
(M4), multi-scope + identity (M5), and importers (M6) are implemented.
Deferred per ADR: real Jira/Confluence MCP adapters, strict/auto-sync
external storage, evidence URL fetching.

## Build

```sh
cargo build --release -p ft-cli --bin firetrail
# Binary lands at target/release/firetrail
```

For real semantic search (instead of the deterministic mock embedder):

```sh
cargo build --release -p ft-cli --bin firetrail --features ft-embed/onnx
```

The default build is hermetic (no native ONNX dependency). Without
`--features onnx`, `firetrail daemon start` prints a warning and falls back
to the mock embedder. Lexical search still works fully.

## Quick start

```sh
# 1. Scaffold a workspace inside an existing git repo
firetrail init
firetrail doctor

# 2. Track work
firetrail epic --title "Ship v1"
firetrail task --title "Build auth" --epic <epic-id>
firetrail criteria add <task-id> "Login works"
firetrail ready                       # unblocked work
firetrail claim <task-id>
firetrail board
firetrail close <task-id>

# 3. Capture memory (M2)
firetrail finding --title "Redis OOM under spike" --description "…"
firetrail incident --title "Checkout 500s on Tuesday"
firetrail verify                      # history-chain integrity

# 4. Search and prime an agent (M3)
firetrail daemon start                # background daemon (Unix only)
firetrail search "redis"
firetrail prime --task <task-id>      # build a context pack
firetrail daemon stop

# 5. PR safety (M4)
firetrail merge-driver-install
firetrail check pr
firetrail lint memory --fix           # remediation hints

# 6. Multi-scope, identities (M5)
firetrail identity register --id alice --email alice@example.com
firetrail scope list
firetrail claim-takeover <task-id>    # admins only on live claims

# 7. Import historical markdown (M6)
firetrail import incidents docs/incidents/
firetrail promote-import --interactive
```

Run `firetrail --help` for the full command surface.

## Working in this repo

```sh
just            # build + test + clippy + fmt-check
just test       # tests via cargo-nextest if present
just lint       # clippy -D warnings
```

Issue tracking lives in `bd` (beads). Run `bd ready` to see open work.

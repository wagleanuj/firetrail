## Firetrail workflow

This repo tracks work, decisions, and incident memory as JSON records under
`.firetrail/records/`. Every change to that graph goes through the
`firetrail` CLI. Read this section before issuing any task command.

### Mandatory loop

```
firetrail doctor                       # verify workspace health (run first)
firetrail ready                        # find unblocked work
firetrail show <id>                    # read the full record before claiming
firetrail claim <id>                   # acquire exclusive lock (7d default)
# … do the work …
firetrail close <id>                   # acceptance criteria must be complete
```

Never start work without claiming. Concurrent claims on the same record
fail with a Conflict exit code — that's the system catching a collision,
not a bug. Resolve by coordinating with the current claimant or running
`firetrail claim-takeover <id>` (admin-gated).

### Creating work

```
firetrail epic    --title "Ship v1"                        # parent for tasks
firetrail task    --title "Build auth" --epic <epic-id>
firetrail subtask --title "Login form" --parent <task-id>
firetrail bug     --title "OAuth callback 500"
firetrail criteria add <id> "Login works on Safari 17"
firetrail dep add <id> --blocks <blocker-id>               # graph edges
firetrail link <id> --evidence "<url>" --kind pr           # external proof
```

Every task/subtask/bug **must** declare acceptance criteria before close.
`firetrail close` refuses to close incomplete records; `--force --reason`
is reserved for genuine exceptions and is audited.

### Recording knowledge (M2 records)

When you discover something worth keeping — a production gotcha, a load
shed pattern, a decision rationale — write a record. The corpus is what
makes future agents (and humans) effective.

```
firetrail finding  --title "Redis OOM under spike traffic"  --description "..."
firetrail incident --title "Checkout 500s on 2026-05-12"
firetrail runbook  --title "Rotate Stripe webhook secret"
firetrail decision --title "Use Postgres advisory locks for idempotency"
firetrail gotcha   --title "TLS verify off in staging only"
firetrail capture  --kind finding --title "..."             # quick capture
```

Trust states progress `draft → reviewed → verified`. Drafts older than
the configured threshold get flagged by `firetrail lint memory`. Don't
mark records `verified` yourself unless your identity has the
`can_promote_verified` capability (check via `firetrail identity show`).

### Searching and priming context

```
firetrail search "redis oom"            # hybrid lexical + vector
firetrail similar <id>                  # nearest records to <id>
firetrail prime --task <id>             # build an agent context pack
firetrail prime --query "..."           # ad-hoc context pack
```

`prime` enforces a token budget and emits an `omitted` manifest listing
what was cut. Always inspect that manifest — important context may have
been trimmed.

The vector path runs through the embedding daemon; if it isn't running,
`firetrail daemon start` spawns it in the background. Default builds use
a deterministic mock embedder; semantic search needs
`--features ft-embed/onnx` at build time.

### Before opening a PR

```
firetrail check pr                      # full validator suite
firetrail lint memory --fix             # remediation hints (non-mutating)
firetrail verify                        # history-chain integrity
firetrail diff <base>..<head>           # record-aware diff
```

`check pr` enforces evidence on trust transitions, acceptance-criteria
completion, memory-only PR rules (memory records cannot ship in a code
PR; ADR-0009), and chain integrity. A failing check is the system
preventing real damage — fix the cause, never bypass with `--force`.

### What NOT to do

- **Never edit `.firetrail/records/**/*.json` by hand.** The `state_hash`
  + `prev_state_hash` chain (ADR-0017) detects tampering and your edit
  will be caught by `firetrail verify`. Use the CLI exclusively.
- **Never delete records.** Use `firetrail memory archive <id>` or
  `supersede <id> --with=<new-id>`. Deletion breaks the history chain.
- **Never skip `firetrail check pr`.** It's the last guard.
- **Never bypass `--no-verify` on git hooks.** The hooks enforce the
  chain. If a hook fails, fix the underlying record, don't dodge it.

### Where the design lives

- `docs/ARCHITECTURE.md` — system overview
- `docs/ROADMAP.md` — milestone definitions and gates
- `docs/decisions/` — ADRs (read in order; later ADRs supersede earlier)
- `docs/components/ft-*.md` — per-crate specifications
- `firetrail --help` — current command surface (authoritative)
- `firetrail doctor` — workspace health (run when anything feels off)

### Identity

Agents inherit the operator's git identity by default. To register an
agent identity explicitly:

```
firetrail identity register --id claude-bot --emails claude@example.com --kind agent
```

If the workspace runs in strict-identity mode (`identity.strict: true` in
`.firetrail/config.yml`), records authored by unregistered identities are
rejected at commit time.


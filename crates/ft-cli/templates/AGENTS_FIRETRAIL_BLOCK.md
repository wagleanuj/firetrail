## Firetrail workflow

This repo tracks work, decisions, and incident memory as JSON records under
`.firetrail/records/`. Every change to that graph goes through the
`firetrail` CLI. Read this section before issuing any task command.

**Authoritative help is always `firetrail <subcommand> --help`** — the
examples below match the CLI at the time of writing, but `--help` is
canonical. Run it before guessing syntax.

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
fail with a Conflict exit code (3) — that's the system catching a
collision, not a bug. Resolve by coordinating with the current claimant
or running `firetrail claim-takeover <id>` (admin-gated).

### Creating work

All work-graph creators use a `create` subcommand with the title as a
positional argument:

```
firetrail epic    create "Ship v1"                              # parent for tasks
firetrail task    create "Build auth" --epic <epic-id>
firetrail subtask create "Login form" --parent <task-id>
firetrail bug     create "OAuth callback returns 500"
firetrail criteria add <id> "Login works on Safari 17"
firetrail dep      add <from-id> <to-id>                        # from depends on to
firetrail link <from> <to> --type blocks                        # blocks|blocked-by|parent-of|child-of|related-to
```

Every task/subtask/bug **must** declare acceptance criteria before close.
`firetrail close` refuses to close incomplete records; `--force --reason`
is reserved for genuine exceptions and is audited.

### Recording knowledge (M2 records)

When you discover something worth keeping — a production gotcha, a load
shed pattern, a decision rationale — write a record. The corpus is what
makes future agents (and humans) effective.

Each memory kind has its own create surface. Note the **summary** vs
**title** distinction: findings/incidents/gotchas take a positional
summary; runbooks/decisions take a positional title plus required flags.

```
firetrail finding  create "Redis OOM under spike traffic"
firetrail incident create "Checkout 500s on 2026-05-12"
firetrail runbook  create "Rotate Stripe webhook secret" --summary "When and how"
firetrail decision create "Use Postgres advisory locks" \
                          --context "..." --decision "..."
firetrail gotcha   create "TLS verify off in staging only"
firetrail capture  --kind finding --title "Quick observation"    # opportunistic
```

Trust states progress `draft → reviewed → verified`. Promote with the
dedicated commands:

```
firetrail memory review    <id>        # draft → reviewed
firetrail memory promote   <id>        # reviewed → verified (capability-gated)
firetrail memory deprecate <id>
firetrail memory archive   <id>
firetrail memory supersede <id> --with <successor-id>
firetrail memory merge     <id1> <id2> ... --into <canonical-id>
firetrail memory stale                 # records past freshness threshold
```

Don't mark records `verified` yourself unless your identity has the
`can_promote_verified` capability — check via `firetrail identity show <id>`.

### Searching and priming context

```
firetrail search "redis oom"                       # hybrid lexical + vector
firetrail similar <id>                             # nearest records to <id>
firetrail prime --task <id>                        # context pack for a task
firetrail prime --query "checkout latency"         # ad-hoc context pack
firetrail prime --query "..." --max-tokens 8000 --min-trust reviewed
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
firetrail check pr <base-ref> <head-ref>   # full validator suite
firetrail lint memory --fix                # remediation hints (non-mutating)
firetrail verify                           # history-chain integrity, all records
firetrail verify <id>                      # verify a single record
firetrail diff <base-ref> <head-ref>       # record-aware diff
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
  `memory supersede <id> --with <new-id>`. Deletion breaks the history
  chain.
- **Never skip `firetrail check pr`.** It's the last guard.
- **Never bypass `--no-verify` on git hooks.** The hooks enforce the
  chain. If a hook fails, fix the underlying record, don't dodge it.
- **Never trust example syntax over `--help`.** Templates drift; the CLI
  is the source of truth. When in doubt: `firetrail <cmd> --help`.

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
firetrail identity register <id> --name "Claude Bot" \
                                 --emails claude@example.com \
                                 --kind agent
firetrail identity list
firetrail identity show <id>
```

If the workspace runs in strict-identity mode (`identity.strict: true` in
`.firetrail/config.yml`), records authored by unregistered identities are
rejected at commit time.


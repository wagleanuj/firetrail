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
firetrail search "<task keywords>"     # has this been solved before?
firetrail prime --task <id>            # build a context pack for this task
firetrail claim <id>                   # acquire exclusive lock (7d default)
# … do the work …
# (capture findings/gotchas/decisions as they surface — see below)
firetrail close <id>                   # acceptance criteria must be complete
```

Never start work without claiming. Concurrent claims on the same record
fail with a Conflict exit code (3) — that's the system catching a
collision, not a bug. Resolve by coordinating with the current claimant
or running `firetrail claim-takeover <id>` (admin-gated).

**Always recall before you build.** `search` + `similar` + `prime` are
the difference between a fresh agent and an agent with institutional
memory. If a finding already exists for the failure mode you're about
to investigate, you save the hour. If a decision already exists for
the architectural call you're about to make, you stay consistent.

### Creating work

All work-graph creators use a `create` subcommand with the title as a
positional argument:

```
firetrail epic    create "Ship v1"                              # parent for tasks
firetrail task    create "Build auth" --epic <epic-id>
firetrail subtask create "Login form" --parent <task-id>
firetrail bug     create "OAuth callback returns 500"
firetrail criteria add <id> "Login works on Safari 17"
```

Every task/subtask/bug **must** declare acceptance criteria before close.
`firetrail close` refuses to close incomplete records; `--force --reason`
is reserved for genuine exceptions and is audited.

### Wiring dependencies (mandatory for any non-trivial epic)

**If an epic contains more than one task, you MUST wire the dependency
graph before anyone — human or agent — runs `firetrail ready`.** Without
edges, every task appears unblocked and parallel claims will produce
work that has to be unwound (you can't build `useWeather` before the
API module exists). The graph is what makes `firetrail ready` useful.

Two relation surfaces, used for different things:

```
firetrail dep  add <from-id> <to-id>           # <from> depends on <to>
                                                # (<to> blocks <from>)
firetrail link <from> <to> --type blocks       # blocks | blocked-by
                                                # | parent-of | child-of
                                                # | related-to
```

Rule of thumb:
- **`dep add`** — execution dependency. A task that cannot start until
  another finishes. This is what drives `firetrail ready`.
- **`link --type related-to`** — informational relation (this task
  references that finding, this incident invoked that runbook). Does
  not gate readiness.

Workflow when you break an epic into tasks:

1. Create the epic and all tasks first (titles only).
2. Map the dependency graph on paper or in your head.
3. `firetrail dep add` each edge in **leaf-to-root** order so you don't
   reference an id that doesn't yet exist.
4. `firetrail graph <epic-id>` — visually verify the shape.
5. `firetrail ready` — confirm only the genuine leaves are unblocked.
6. Now you may claim.

A flat list of siblings under an epic is almost always a bug. If you
truly believe the tasks are independent, say so explicitly in the epic
description so reviewers know it was a decision, not an oversight.

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

**When to capture (not optional — this is how the corpus grows):**

| Trigger | Record kind |
|---|---|
| A bug you fixed turned out to have a non-obvious cause | `finding create` |
| A production incident happened (live or recent) | `incident create` |
| A reproducible "do these N steps" procedure emerged | `runbook create` |
| You made an architectural call that future agents need to honour | `decision create` |
| A subtle trap that bit you and will bite the next person | `gotcha create` |
| Quick observation, can't decide kind yet | `capture --kind memory` |

Capture *during* the work, not after. The next agent priming this task
will read what you wrote — make it count. Aim for one finding or one
decision per non-trivial task; runbooks and gotchas come opportunistically.

**Importing existing markdown docs (post-mortems, ADRs, runbooks).**
The one-line `incident|decision|runbook create` commands are for
brand-new records typed at the prompt — they accept a summary string
but no body. If the user hands you an existing markdown file (an RCA,
a past post-mortem, a draft ADR, a runbook), do NOT retype it into
`create`. Use the importer:

```
firetrail import incidents <dir>     # post-mortems / RCAs
firetrail import adrs      <dir>     # ADRs / decisions
firetrail import runbooks  <dir>     # operational runbooks
```

The importer parses standard sections (summary, root cause, resolution,
action items, lessons learned for incidents; context/decision/
consequences for ADRs; steps + applies-to for runbooks), preserves the
full markdown body, and reports a `parse_confidence` per file. For a
single file from chat or a paste, drop it into a tempdir first — the
importer expects a directory. **Memories are immutable**, so a field
missed at create time cannot be patched in later; the importer is the
only path that captures structured fields like `root_cause`.

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


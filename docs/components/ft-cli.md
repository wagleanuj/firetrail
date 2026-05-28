# ft-cli — command-line interface (M1 surface)

**Epics:** `firetrail-s45` (scaffold + init/doctor), `firetrail-1xc` (work-graph commands)
**Wave:** 3 (scaffold), 4 (commands)
**Depends on:** ft-core, ft-storage, ft-identity, ft-index, ft-git
**Binary name:** `firetrail`

---

## Purpose

`ft-cli` is the user-facing binary. It glues the other crates together, parses
arguments via `clap`, and produces output in the user's chosen format (markdown
for terminals, JSON for scripts and agents).

This spec covers the M1 command surface only. M2+ commands (memory, search,
prime, check pr, import, etc.) extend `ft-cli` later.

---

## Global options

```
firetrail [GLOBAL OPTIONS] <command> [COMMAND ARGS]

Global options:
  --format markdown|json     Output format. Auto-detects: markdown on a TTY, json otherwise.
  --json                     Shortcut for --format json.
  --quiet, -q                Suppress non-essential output.
  --verbose, -v              Enable verbose diagnostics.
  --workspace <path>         Override workspace root (default: discover from cwd).
  --help, -h                 Show help.
  --version                  Show version.
```

Every command accepts `--help` describing its arguments and exit codes.

---

## Exit codes

```
0    Success
1    User error (invalid arguments, validation failed, AC incomplete)
2    Not found (record, workspace, branch)
3    Conflict (claim conflict, hash mismatch, stale data)
4    Workspace not initialized
5    Internal error (bug; printed with a stack trace if --verbose)
```

JSON output on error includes `error.code` matching the exit code numerically
and `error.kind` matching its symbolic name (e.g. `user_error`).

---

## Output format

### Markdown (default for TTY)

Human-readable. Headers, lists, tables (using `comfy-table` or similar). Color
via `nu_ansi_term` when stdout is a TTY.

### JSON (default for non-TTY)

Stable, documented schema per command. Wrapped in:

```json
{
  "format_version": 1,
  "command": "task create",
  "data": { ... },
  "warnings": [],
  "elapsed_ms": 42
}
```

Errors:

```json
{
  "format_version": 1,
  "command": "close",
  "error": {
    "code": 1,
    "kind": "user_error",
    "message": "acceptance criteria incomplete",
    "details": { "incomplete": [{"id": "ac-02", "text": "..."}] }
  }
}
```

---

## Commands (M1)

### Bootstrap

```
firetrail init [--storage-mode embedded|external] [--strict-identity]
firetrail doctor [--network] [--fix]
```

#### `firetrail init`

- Verifies the cwd is inside a git repo.
- Creates `.firetrail/` with `config.yml` and `records/<type>/` directories.
- Initializes the index database.
- Installs git hooks (`pre-commit`, `post-checkout`, `post-merge`).
- Adds `.firetrail/index.db` and `.firetrail/cache/` to `.gitignore`.
- Writes `AGENTS.md` and `.claude/skills/firetrail/SKILL.md` (skill is created
  even though full skill content is M2+; M1 ships a minimal skill pointing at
  the CLI).
- Honors `--storage-mode external` by prompting (or accepting flags) for the
  data repo URL; at M1 only embedded is enforced. External returns "not yet
  available" and falls through to embedded.
- Honors `--strict-identity` by setting `identity.strict: true` in config.

Idempotent: re-running on an initialized workspace updates hooks and config in
place, preserving user customizations.

#### `firetrail doctor`

Reports:

- Workspace presence and version.
- Config validity.
- Identity resolution (the trace from `ft-identity::resolve_with_trace`).
- Git status (clean, current branch, detached HEAD detection).
- Hook installation status.
- Index database integrity (`PRAGMA integrity_check`).
- Index freshness vs. current HEAD.
- Storage integrity: counts of records on disk vs. records in the index.
- Schema version vs. binary's expected version.

Each item prints `OK`, `WARN`, or `FAIL` with an actionable suggestion. With
`--fix`, runs safe remediations (rebuild index, install missing hooks).

`--network` enables checks for integrations (M5+). At M1 it's a no-op with a
note that network checks become meaningful in later milestones.

### Work-graph creation

```
firetrail epic create <title> [--description <text>] [--priority p0|p1|p2|p3|p4] \
                              [--scope <s>] [--label key=value ...]

firetrail task create <title> [--description <text>] [--epic <id>] \
                              [--priority p0|p1|p2|p3|p4] [--owner <identity>] \
                              [--scope <s>] [--label key=value ...]

firetrail subtask create <title> --parent <task-id> [--description <text>] ...

firetrail bug create <title> [--service <s>] [--severity sev1|sev2|sev3] ...
```

All `create` commands print the new record's ID. JSON output includes the full
record envelope.

### Work-graph mutation

```
firetrail update <id> [--title <t>] [--status <s>] [--priority <p>] [--owner <i>] \
                      [--description <d>] [--scope <s>] [--label key=value ...]

firetrail close <id> [--force --reason <text>]
firetrail reopen <id>

firetrail claim <id> [--expires <duration>]
firetrail unclaim <id>
```

`close` validates that acceptance criteria are complete (every AC has
`status == Checked`). Refuses with exit 1 listing incomplete ACs. `--force`
overrides; `--reason` is required when forcing.

`claim` mints a `Claim` with mandatory `claim_expires_at`. Default duration is
7 days; configurable per workspace; overridable per claim via `--expires`.

`unclaim` releases a claim if held by the resolver's identity. Releasing
another actor's claim requires `--takeover --reason <text>` (M5 feature; M1
prints "not yet supported" and exits with code 1).

### Acceptance criteria

```
firetrail criteria add <id> "<text>"
firetrail criteria list <id>
firetrail criteria check <id> <ac-id-or-index>
firetrail criteria uncheck <id> <ac-id-or-index>
firetrail criteria evidence <id> <ac-id-or-index> --url <url>
```

`<ac-id-or-index>` accepts either the AC's local id (e.g. `ac-02`) or its
1-based index in the AC list.

### Dependencies and links

```
firetrail link <from> <to> [--type related-to|fixed-by|caused-by|...]
firetrail dep add <id> <blocked-by-id> [--type blocked-by|blocks|parent-of|...]
firetrail dep remove <id> <other-id> [--type ...]
```

The `dep` subcommand is shorthand for `link` with the common dependency types
and additional validation (e.g. refuses self-edges).

### Views

```
firetrail show <id> [--include-history] [--include-relations]
firetrail list [--type <t>] [--status <s>] [--owner <i>] [--scope <s>] \
               [--label key=value ...] [--limit <n>] [--offset <n>]
firetrail ready [--type <t>] [--owner <i>] [--scope <s>] [--limit <n>]
firetrail board [--scope <s>] [--owner <i>]
firetrail graph <id> [--direction up|down|both] [--depth <n>]
```

#### `firetrail board`

ASCII table:

```
TODO              IN PROGRESS         REVIEW            DONE
─────────────────────────────────────────────────────────────────────
TASK-7f2a91       TASK-9c4b2e         TASK-a3f001       TASK-bb12cc
Add Redis alert   Refactor cache      Fix auth bug      Add logs
P1 @alice         P1 @bob             P2 @carol         P2 @dave
```

Columns map from `Status`:

- `TODO`: Open + Ready
- `IN PROGRESS`: InProgress + Blocked
- `REVIEW`: Review
- `DONE`: Closed (filtered to recent N or `--all-time`)

#### `firetrail graph`

Renders as an ASCII tree using box-drawing characters. Depth defaults to 3.
Direction defaults to `both`.

---

## Command dispatch architecture

```rust
// crates/ft-cli/src/main.rs

#[derive(clap::Parser)]
struct Cli {
    #[command(flatten)]
    global: GlobalOpts,
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand)]
enum Command {
    Init(InitArgs),
    Doctor(DoctorArgs),
    Epic(EpicSubcommand),
    Task(TaskSubcommand),
    Subtask(SubtaskSubcommand),
    Bug(BugSubcommand),
    Update(UpdateArgs),
    Close(CloseArgs),
    Reopen(ReopenArgs),
    Claim(ClaimArgs),
    Unclaim(UnclaimArgs),
    Criteria(CriteriaSubcommand),
    Link(LinkArgs),
    Dep(DepSubcommand),
    Show(ShowArgs),
    List(ListArgs),
    Ready(ReadyArgs),
    Board(BoardArgs),
    Graph(GraphArgs),
}
```

Each command is dispatched to a function in its own module under
`crates/ft-cli/src/commands/`. Modules return a typed `CommandResult` that the
output layer renders as markdown or JSON.

```rust
pub enum CommandResult {
    Record(Record),
    Records(Vec<IndexedRecord>),
    BoardView(BoardData),
    GraphView(GraphData),
    Acknowledgement { message: String, details: serde_json::Value },
}
```

---

## Acceptance (E-M1-08, scaffold + init/doctor)

1. `firetrail init` in an empty git repo produces a working `.firetrail/` such
   that `firetrail show <any-id>` returns `NotFound`, not `NotInitialized`.
2. `firetrail init` is idempotent: running twice does not corrupt state.
3. `firetrail doctor` on a fresh init reports all checks `OK`.
4. `firetrail doctor` after `rm .firetrail/index.db` reports the integrity
   failure and suggests `firetrail index rebuild` (or with `--fix`, runs the
   rebuild itself).
5. `--json` output is parseable by `jq` for every command, including errors.
6. `--format markdown` output uses no ANSI codes when stdout is piped.
7. `firetrail --help` and `firetrail <cmd> --help` print useful text.
8. Exit codes match the table above for every error path.

## Acceptance (E-M1-09, work-graph commands)

1. Every `create` command writes a record to disk via `ft-storage` and updates
   the index via `ft-index`. The new ID is returned in stdout.
2. `update` modifies the requested fields; `state_hash` is recomputed; the
   index reflects the change after the command returns.
3. `close` refuses with exit 1 when ACs are incomplete; the error JSON lists
   the incomplete ACs.
4. `close --force --reason <t>` succeeds and writes the reason into the
   record's history entry.
5. `claim` is atomic: concurrent invocations on the same record (simulated via
   two child processes) produce exactly one success and one conflict error.
6. `criteria add/check/uncheck` works through the AC list; index reflects the
   new state immediately.
7. `link` and `dep add` write relations; `dep remove` removes them.
8. `show`, `list`, `ready`, `board`, `graph` all produce stable
   markdown-snapshot output (verified with `insta`).
9. Every command emits JSON output documented in the JSON schema export.

---

## Testing requirements

- Per-command integration tests using `ft-testkit::TestRepo::firetrail()` to
  invoke the binary.
- Insta snapshot tests for `board`, `graph`, and `show` markdown output.
- JSON schema tests: every command's `--json` output validates against a
  documented schema in `docs/schema/cli-output-v1.json`.
- Concurrency test for `claim` using two child processes.
- Doc tests on the main command functions.

---

## Out of scope (deferred)

- All memory commands (`finding create`, `incident create`, etc.) — M2.
- `capture`, `verify`, `memory salvage`, `memory promote-to-main` — M2.
- `search`, `similar`, `prime` — M3.
- `check pr`, `lint memory`, `diff --memory` — M4.
- `identity` registry commands — M5.
- `import incidents|adrs|runbooks|refresh`, `promote-import` — M6.
  Jira/Confluence/GitHub adapters are out of scope (ADR-0014 addendum);
  the calling agent fetches via its own MCP servers and pipes markdown
  into `firetrail import`.

---

## References

- ADR-0011 — Offline-first contract (every M1 command is offline)
- ADR-0012 — Skill as agent documentation (init writes the skill)
- ADR-0015 — Hash-based IDs (display uses short form, references use full or
  unambiguous prefix)
- ADR-0016 — Build approach

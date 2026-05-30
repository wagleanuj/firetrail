# Firetrail — Agent Build Driver

This file is the entry point for any AI agent (Claude Code, Cursor, or subagent) working on
the Firetrail codebase. Read it before doing anything. It tells you what we are building,
how we work, where decisions live, and how to validate that what you produced is correct.

---

## 1. What we are building

**Firetrail** is a repo-native work graph and incident memory system for engineering teams.
Tasks, incidents, findings, runbooks, decisions, and memory records live as JSON files in
Git. SQLite + sqlite-vec is a derived read index. Engineers and AI agents read structured
context via `firetrail prime`, write records via the CLI, and review changes through PRs.

Firetrail itself never calls an LLM at runtime. The reasoning layer is the host agent
(Claude Code, Cursor, or a human). Firetrail provides structured context and structural
guardrails.

For the original brief, see `requirements.md`. For the current design, read `docs/`.

---

## 2. The build approach

**One human plus AI subagents. Built in Rust. Parallel where possible, validated everywhere.**

### Workspace layout

A Cargo workspace with many small crates. Each crate is sized to fit one agent's context
(~2k–5k lines). See `docs/decisions/0016-build-approach.md` for the full rationale.

```
crates/
  ft-core/       record types, schema, hash chain
  ft-storage/    JSON-in-Git read/write, embedded + external modes
  ft-index/      SQLite + sqlite-vec read index
  ft-embed/      ONNX daemon, embedding cache
  ft-identity/   registry, resolution, capabilities
  ft-scope/      multi-scope routing, CODEOWNERS resolution
  ft-trust/      trust state machine, evidence, review workflow
  ft-history/    PR-time compaction, prev_state_hash chain
  ft-search/     vector + lexical + ranking
  ft-prime/      context pack generation
  ft-pr/         check pr, custom merge driver
  ft-import/     markdown, jira, confluence importers
  ft-git/        git operations wrapper
  ft-cli/        clap entry, command dispatch
  ft-testkit/    shared test fixtures, factories
```

Crates are built in waves. Within a wave, crates are independent and can be implemented
by separate subagents in parallel git worktrees.

```
Wave 1 (foundation):
  ft-core, ft-git, ft-testkit

Wave 2 (parallel):
  ft-storage, ft-identity, ft-history

Wave 3 (parallel):
  ft-index, ft-embed, ft-scope, ft-trust

Wave 4 (parallel):
  ft-search, ft-prime, ft-import, ft-pr

Wave 5:
  ft-cli (glue layer)
```

### Subagent assignment

Each subagent receives:

1. The component spec from `docs/components/<crate>.md`.
2. Relevant ADRs linked from the spec.
3. The crate skeleton with public API stubbed.
4. The list of tests that must pass.
5. **Constraint: do not modify other crates' code.** Public APIs only.

The agent implements until the test list passes. It may add tests if it finds gaps in
coverage. It must not skip the verifier step (see below).

### Verifier-agent pattern

For every component PR, a second subagent runs as an independent reviewer with this brief:

> You did not write this code. Read the spec, the ADRs, and the diff. Without consulting
> the author's tests, write three additional tests the implementation should pass. Run
> them. Report results.

This catches the AI-claims-done-on-broken-code failure mode. Implementer and verifier have
different prompts and therefore make different mistakes.

---

## 3. Where to find things

```
requirements.md                Original brief. Historical reference only.

docs/
├── ARCHITECTURE.md            How the system fits together (entry point — read first)
├── ROADMAP.md                 Milestones, gates, success criteria per milestone
├── DOCS.md                    Convention for linking docs to work (frontmatter, doc_type)
├── BUILD_PLAN.md              Phased implementation plan
├── decisions/                 ADRs — why we chose what we chose
│   ├── 0001-rust-over-go.md
│   ├── 0002-json-in-git-not-dolt.md
│   ├── 0003-pr-compaction-history.md
│   ├── 0004-multi-scope-records.md
│   ├── 0005-no-llm-in-tool.md
│   ├── 0006-storage-modes.md
│   ├── 0007-local-embeddings-daemon.md
│   ├── 0008-identity-registry.md
│   ├── 0009-memory-only-prs.md
│   ├── 0010-pr-link-enforcement.md
│   ├── 0011-offline-first.md
│   ├── 0012-skill-as-agent-docs.md
│   ├── 0013-trust-model.md
│   ├── 0014-import-quarantine.md
│   ├── 0015-hash-based-ids.md
│   ├── 0016-build-approach.md
│   ├── 0017-audit-chain-integrity.md
│   ├── 0018-branch-salvage.md
│   └── 0019-prime-context-budget.md
└── components/                Per-crate specs. The contract agents implement against.
    ├── ft-core.md
    ├── ft-storage.md
    └── ...
```

**Reading order for a new agent:**

1. This file.
2. `docs/ARCHITECTURE.md` — the integration view.
3. `docs/decisions/` — read ADRs relevant to your assigned work.
4. `docs/components/<crate>.md` — the spec for your specific crate.
5. The crate's `Cargo.toml` and any existing source.

---

## 4. How to pick up work

All work is tracked in beads, the local issue tracker at `.beads/`. Issue prefix is `firetrail`.

```bash
bd ready                # show unblocked work
bd ready --json         # same, machine-readable
bd show firetrail-<id>  # detail view including dependencies and design notes
bd update <id> --claim  # claim atomically so other agents do not pick the same task
```

**Rules:**

- Always `bd ready` before asking what to work on.
- Always `--claim` before starting. If the claim fails, someone else got there first.
- If you discover related work mid-task, file a new issue with
  `--deps discovered-from:<parent-id>`. Do not silently expand scope.
- Close issues only when validation passes (see Section 5). Use
  `bd close <id> --reason "..."`, or land a commit whose message contains
  `Closes: firetrail-<id>` and the post-commit hook closes it for you.

**Naming and shape:**

- Issues created during the build are prefixed `firetrail-<hash>` (e.g. `firetrail-a3f2dd`).
- Epics group related work. Children roll up to epics via `bd create --parent <epic-id>`.
  Do NOT use `bd dep add child epic` with the default `blocks` type — that inverts the
  relationship and makes the epic block its children.
- For an existing child, attach to the epic with
  `bd dep add <epic> <child> --type=parent-child`.

**Auto-close on commit:**

- `.beads/hooks/post-commit` scans HEAD's commit message for trailer-style
  `Closes: firetrail-<id>` / `Fixes: firetrail-<id>` / `Resolves: firetrail-<id>` lines
  (colon required) and closes each referenced issue. Multiple ids per trailer are fine.
- Opt out for one commit with `BD_NO_AUTOCLOSE=1 git commit ...`.

---

## 5. How to validate work

Five layers of testing. Inner-loop development depends on the first three. CI runs all
five on every PR.

### Layer 0: compile (instant)

Rust's type system is your fastest reviewer. Encode trust transitions, scope routing,
record kinds, and identity capabilities as `enum` and type-state so incorrect transitions
are compile errors rather than runtime bugs.

### Layer 1: unit tests (sub-second per crate)

```bash
cargo nextest run -p ft-<crate>
```

Pure logic, no filesystem, no SQLite, no Git.

### Layer 2: property tests (seconds)

```bash
cargo nextest run -p ft-<crate> --features proptest
```

Use `proptest` for record parsing, scope routing, merge logic, hash chain validation,
trust transitions. Property tests catch the edge cases AI agents predictably miss —
empty arrays, deeply nested input, unicode, malformed fields.

### Layer 3: integration tests (seconds to minutes)

```bash
cargo nextest run --workspace --features integration
```

Real SQLite (tempfile), real Git repos (tempdir), real filesystem. Use
`ft_testkit::TestRepo::new()` for isolated workspaces.

### Layer 4: scenario tests (a few minutes)

```bash
cargo run -p ft-testkit --bin scenarios
```

Black-box CLI tests. Spawn the `firetrail` binary against a fixture repo, execute a
sequence of commands, assert observable state. Scenarios live in
`tests/scenarios/*.scenario`.

### Layer 5: conflict and merge tests (a few minutes)

```bash
cargo nextest run --workspace --features slow-tests
```

Two engineers, two branches, conflicting edits, custom merge driver runs, expected final
state asserted. These catch the concurrency bugs the AI will absolutely miss.

### Validation gates per PR

Before claiming a task as complete, every one of these must pass:

1. `cargo fmt --check`
2. `cargo clippy -- -D warnings`
3. `cargo nextest run` (unit + property + integration)
4. `cargo test --doc`
5. Scenario suite passes
6. Verifier subagent signed off (in PR description)
7. `docs/components/<crate>.md` requirements demonstrably covered by tests

Run the local helper:

```bash
./scripts/validate.sh
```

A pre-commit hook runs items 1–3 locally. CI runs all of them on every PR.

### When tests fail

Do not patch around failures. Find the root cause. If a test failure exposes a spec gap,
update the spec and the ADR before changing the test. Tests describe the contract; the
contract is the source of truth.

---

## 6. Conventions

### Rust style

- 2024 edition. `cargo fmt` with default config.
- `clippy::pedantic` on by default; suppress with comments only when justified.
- Error handling: `thiserror` for library errors, `anyhow` for application code in `ft-cli`.
- Async: `tokio` for the embedding daemon. Synchronous for everything else.
- Public APIs documented with `///` doc comments and `cargo test --doc` examples.
- `#[must_use]` on builder methods and constructors of important state.

### Crate boundaries

- Public API is whatever appears in `lib.rs`. Internal modules are `pub(crate)` by default.
- Do not depend on another crate's internals. If you need something internal, extend the
  spec, then expose it via the spec'd public API.
- Cross-crate types live in `ft-core`. Everything else builds on top.

### Naming

- Records and types: `Record`, `Task`, `Incident`, `Finding` — singular.
- IDs at the type level: `RecordId`, `TaskId`, etc. — newtype wrappers around `String`.
- Database operations: `read_record`, `write_record`, `list_records` — verbs first.
- Errors: `<Domain>Error` — `StorageError`, `IndexError`.

### Commits

- Conventional Commits format (`feat:`, `fix:`, `docs:`, `refactor:`, `test:`).
- One logical change per commit.
- Reference the bd issue in the footer: `firetrail-issue: firetrail-a3f2dd`.

### PR descriptions

Every PR must include:

- Linked bd issue: `firetrail-closes: firetrail-a3f2dd`.
- Summary of what changed and why.
- Test plan: which gates were run locally, which are running in CI.
- Verifier signoff section (filled in by the verifier subagent before merge).

---

## 7. Conventions specifically about how AI agents work in this repo

These are the rules that exist because AI agents are doing most of the implementation.

### Never claim done without evidence

A statement like "the tests pass" must be backed by the actual command output. If you
ran `cargo nextest run` and 47/47 passed, paste the output. Do not summarize.

### Never modify other crates without explicit task

If your spec is `ft-storage` and your work needs a new method on `ft-core`, do not add
it silently. File a new bd issue (`bd create --deps blocks:<your-task-id>`), block on it,
and either implement it as a new task or wait for someone else to.

### Never bypass validation gates

If `cargo clippy` fails on a warning you do not understand, the answer is to understand
it, not to suppress it. The gates exist to catch the specific class of mistakes AI agents
make.

### Run the verifier before claiming done

The verifier-agent pattern (Section 2) is mandatory. The PR description must include the
verifier's three additional tests and their results. A claim of done without verifier
signoff is not done.

### Beads is the only task tracker

- Do not create markdown TODO files.
- Do not use the host environment's TodoWrite/TaskCreate tools.
- Do not invent a parallel tracking system.
- `bd` is the truth.

### Memory and persistent knowledge

Use `bd remember "..."` for insights that should outlive a single session. Do not write
`MEMORY.md` files. Search via `bd memories <keyword>`.

---

## 8. Session close protocol

When ending a work session you MUST:

1. **File issues for remaining work** — anything discovered but not done.
2. **Run quality gates** if code changed — `./scripts/validate.sh`.
3. **Update issue status** — close finished work, update in-progress items.
4. **Commit and push** — `git add`, `git commit`, `git push`. Work that is not pushed
   does not exist for the next session.
5. **Hand off** — final message describes what was done and what is next.

Work is NOT complete until `git push` succeeds.

---

## 9. Open questions, decisions, and discoveries

When you discover something worth recording (a constraint, a design choice, a workaround,
a non-obvious failure mode), do one of three things:

- **Constraint or design choice with broad implications** → propose a new ADR in
  `docs/decisions/`. Number it after the latest existing ADR.
- **Component-level decision** → update the relevant `docs/components/<crate>.md` spec.
- **Insight or workaround that future agents need** → `bd remember "..."`.

Do not bury decisions in commit messages or PR descriptions. Surface them where future
agents will find them.

---

<!-- BEGIN BEADS INTEGRATION v:1 profile:full hash:f65d5d33 -->
## Issue Tracking with bd (beads)

**IMPORTANT**: This project uses **bd (beads)** for ALL issue tracking. Do NOT use markdown TODOs, task lists, or other tracking methods.

### Why bd?

- Dependency-aware: Track blockers and relationships between issues
- Git-friendly: Dolt-powered version control with native sync
- Agent-optimized: JSON output, ready work detection, discovered-from links
- Prevents duplicate tracking systems and confusion

### Quick Start

**Check for ready work:**

```bash
bd ready --json
```

**Create new issues:**

```bash
bd create "Issue title" --description="Detailed context" -t bug|feature|task -p 0-4 --json
bd create "Issue title" --description="What this issue is about" -p 1 --deps discovered-from:bd-123 --json
```

**Claim and update:**

```bash
bd update <id> --claim --json
bd update bd-42 --priority 1 --json
```

**Complete work:**

```bash
bd close bd-42 --reason "Completed" --json
```

### Issue Types

- `bug` - Something broken
- `feature` - New functionality
- `task` - Work item (tests, docs, refactoring)
- `epic` - Large feature with subtasks
- `chore` - Maintenance (dependencies, tooling)

### Priorities

- `0` - Critical (security, data loss, broken builds)
- `1` - High (major features, important bugs)
- `2` - Medium (default, nice-to-have)
- `3` - Low (polish, optimization)
- `4` - Backlog (future ideas)

### Workflow for AI Agents

1. **Check ready work**: `bd ready` shows unblocked issues
2. **Claim your task atomically**: `bd update <id> --claim`
3. **Work on it**: Implement, test, document
4. **Discover new work?** Create linked issue:
   - `bd create "Found bug" --description="Details about what was found" -p 1 --deps discovered-from:<parent-id>`
5. **Complete**: `bd close <id> --reason "Done"`

### Quality
- Use `--acceptance` and `--design` fields when creating issues
- Use `--validate` to check description completeness

### Lifecycle
- `bd defer <id>` / `bd supersede <id>` for issue management
- `bd stale` / `bd orphans` / `bd lint` for hygiene
- `bd human <id>` to flag for human decisions
- `bd formula list` / `bd mol pour <name>` for structured workflows

### Auto-Sync

bd automatically syncs via Dolt:

- Each write auto-commits to Dolt history
- Use `bd dolt push`/`bd dolt pull` for remote sync
- No manual export/import needed!

### Important Rules

- ✅ Use bd for ALL task tracking
- ✅ Always use `--json` flag for programmatic use
- ✅ Link discovered work with `discovered-from` dependencies
- ✅ Check `bd ready` before asking "what should I work on?"
- ❌ Do NOT create markdown TODO lists
- ❌ Do NOT use external issue trackers
- ❌ Do NOT duplicate tracking systems

For more details, see README.md and docs/QUICKSTART.md.

## Session Completion

**When ending a work session**, you MUST complete ALL steps below. Work is NOT complete until `git push` succeeds.

**MANDATORY WORKFLOW:**

1. **File issues for remaining work** - Create issues for anything that needs follow-up
2. **Run quality gates** (if code changed) - Tests, linters, builds
3. **Update issue status** - Close finished work, update in-progress items
4. **PUSH TO REMOTE** - This is MANDATORY:
   ```bash
   git pull --rebase
   bd dolt push
   git push
   git status  # MUST show "up to date with origin"
   ```
5. **Clean up** - Clear stashes, prune remote branches
6. **Verify** - All changes committed AND pushed
7. **Hand off** - Provide context for next session

**CRITICAL RULES:**
- Work is NOT complete until `git push` succeeds
- NEVER stop before pushing - that leaves work stranded locally
- NEVER say "ready to push when you are" - YOU must push
- If push fails, resolve and retry until it succeeds

<!-- END BEADS INTEGRATION -->

# Firetrail Agent-Skill Suite Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the single `firetrail` SKILL.md with a hybrid agent-skill suite — one always-on router + four trigger-activated deep skills — shipped into target repos by `firetrail init`.

**Architecture:** Skill markdown files are embedded in the `firetrail` binary via `include_str!` (matching the existing `templates/SKILL.md` and `templates/hooks/*` pattern) and written into the target repo's `.claude/skills/<name>/SKILL.md` by `init`. `init.rs` changes from writing one file to looping over a static table of `(relative_path, body)` pairs. The `AGENTS.md` managed block gains one pointer line; no procedural content is duplicated between layers.

**Tech Stack:** Rust (clap CLI), `include_str!` compile-time embedding, integration tests via `ft-testkit::TestRepo` spawning the real binary.

**Spec:** `docs/superpowers/specs/2026-05-30-firetrail-agent-skill-suite-design.md`
**Beads epic:** `firetrail-uz0x`

---

## File Structure

**New template files (each a complete `SKILL.md`):**
- `crates/ft-cli/templates/skills/firetrail/SKILL.md` — router (replaces old `templates/SKILL.md`)
- `crates/ft-cli/templates/skills/firetrail-bootstrap/SKILL.md` — A
- `crates/ft-cli/templates/skills/firetrail-epic-breakdown/SKILL.md` — B
- `crates/ft-cli/templates/skills/firetrail-knowledge/SKILL.md` — C
- `crates/ft-cli/templates/skills/firetrail-pr-safety/SKILL.md` — D

**Modified:**
- `crates/ft-cli/templates/AGENTS_FIRETRAIL_BLOCK.md` — add one pointer line (Task 6)
- `crates/ft-cli/src/commands/init.rs` — replace single-file skill write with a table-driven loop; replace `default_skill_md()` with `skill_templates()` (Task 7)
- `crates/ft-cli/Cargo.toml` — no change (drift-guard test uses manual scan, no new dep)

**Deleted (Task 7):**
- `crates/ft-cli/templates/SKILL.md` — superseded by `templates/skills/firetrail/SKILL.md`

**Tests:**
- `crates/ft-cli/tests/init_and_doctor.rs` — extend existing skill test + add multi-skill, `--no-agents`, and drift-guard tests (Tasks 8-9)

**Ordering rationale:** template files (Tasks 1-5) must exist before `init.rs` references them via `include_str!` (compile-time). The old `templates/SKILL.md` and its `include_str!` stay in place until Task 7 so the crate keeps compiling between tasks.

---

## Task 1: Router skill — `firetrail/SKILL.md`

**Files:**
- Create: `crates/ft-cli/templates/skills/firetrail/SKILL.md`

- [ ] **Step 1: Create the router skill file**

Write exactly this content (frontmatter `description` is the trigger contract — keep verbatim):

```markdown
---
name: firetrail
description: Use the firetrail CLI for work-graph queries, dependency wiring, memory capture, recall, and PR validation in this repo. The always-on entry point — it routes to the deeper firetrail-* skills for bootstrap, epic breakdown, knowledge capture, and PR safety. Read AGENTS.md for the comprehensive cross-tool driver.
---

# Firetrail skill (router)

This repo uses `firetrail` for task tracking, dependency graphs, incident
memory, search/recall, and PR safety. **`AGENTS.md` at the repo root is the
canonical cross-tool driver — read it before real work.** This skill is the
Claude Code entry point: it carries the mandatory loop and routes you to the
deep skills for complex jobs.

## Which skill do I need now?

| If you are… | Use skill |
|---|---|
| in a fresh/empty workspace (`firetrail ready` returns nothing, no roadmap) | `firetrail-bootstrap` |
| breaking an epic into 2+ tasks | `firetrail-epic-breakdown` |
| done with work, capturing knowledge, or handed an existing ADR/RCA/runbook | `firetrail-knowledge` |
| about to open, finish, or review a PR | `firetrail-pr-safety` |

## Mandatory loop

```
firetrail doctor                    # verify workspace health (run first)
firetrail ready                     # find unblocked work
firetrail show <id>                 # read the full record before claiming
firetrail search "<task keywords>"  # has this been solved before?
firetrail prime --task <id>         # build a context pack for this task
firetrail claim <id>                # acquire exclusive lock
# … do the work; capture findings/decisions as they surface …
firetrail close <id>                # acceptance criteria must be complete
```

## The five things agents most often get wrong

1. **A flat list of sibling tasks under an epic with no dependency edges.**
   If an epic has 2+ tasks, wire `firetrail dep add <from> <to>` for every
   execution dependency before anyone runs `firetrail ready`. See
   `firetrail-epic-breakdown`.
2. **Claiming work without recall first.** Always `firetrail search` and
   `firetrail prime --task <id>` before `firetrail claim`.
3. **Finishing work without capturing what was learned.** Every non-trivial
   task should produce a finding, decision, or gotcha. See `firetrail-knowledge`.
4. **Guessing CLI syntax from memory.** Run `firetrail <subcommand> --help`
   first — templates drift; `--help` is canonical.
5. **Editing `.firetrail/records/**/*.json` by hand or bypassing hooks with
   `--no-verify`.** The hash chain catches tampering. Use the CLI exclusively.

## Authoritative help

```
firetrail --help                 # full subcommand surface
firetrail <subcommand> --help    # canonical syntax
firetrail doctor                 # workspace health
```

## Common gotchas

- Default builds use the mock embedder; semantic search needs
  `--features ft-embed/onnx` at build time. Lexical search always works.
- The daemon socket lives under `~/.cache/firetrail/<repo-hash>/`.
  `firetrail daemon start` spawns it as needed.
- Memory records cannot ride a code PR — open a separate memory-only PR
  (ADR-0009). See `firetrail-pr-safety`.
- Concurrent claims on one record exit with code 3 (Conflict) — coordinate
  or `firetrail claim-takeover`.
- Trust progression is `draft → reviewed → verified`; you cannot self-promote
  to `verified` without `can_promote_verified` (`firetrail identity show <id>`).

See `AGENTS.md` for the comprehensive workflow and design references.
```

- [ ] **Step 2: Verify the file is well-formed**

Run: `head -5 crates/ft-cli/templates/skills/firetrail/SKILL.md`
Expected: shows `---`, `name: firetrail`, the `description:` line, `---`.

- [ ] **Step 3: Commit**

```bash
git add crates/ft-cli/templates/skills/firetrail/SKILL.md
git commit -m "feat(ft-cli): add router skill template for skill suite"
```

---

## Task 2: Bootstrap skill — `firetrail-bootstrap/SKILL.md`

**Files:**
- Create: `crates/ft-cli/templates/skills/firetrail-bootstrap/SKILL.md`

- [ ] **Step 1: Create the bootstrap skill file**

Write exactly this content:

```markdown
---
name: firetrail-bootstrap
description: Use when starting in a firetrail repo that has no roadmap or architecture docs and no open work — a fresh or empty workspace. Asks the user for consent, then guides a self-contained setup session that drafts ARCHITECTURE.md and ROADMAP.md, adopts them as Doc records, and seeds the first epic and tasks with dependency edges.
---

# Firetrail bootstrap (fresh-repo setup)

Use this when you land in a firetrail workspace that looks empty. Do NOT
auto-generate anything — detect, then ASK the user before acting.

## 1. Detect (conservative)

```
firetrail doctor          # workspace healthy?
firetrail ready           # any unblocked work?
firetrail list            # any work records at all?
```
Also check for `docs/ROADMAP.md` and `docs/ARCHITECTURE.md`.

Treat the workspace as **fresh** only when there are no open/active work
records AND no roadmap doc. If the repo already has work or a roadmap, this
skill does not apply — return to the router and use `firetrail ready`.

## 2. Ask for consent (always)

If fresh, ask the user something like:

> "This firetrail workspace has no roadmap or architecture docs and no open
> work. Want me to help set up the project — draft an architecture overview
> and a roadmap, then seed the first work items? (I'll ask a few questions.)"

If they decline, stop. Do not create files or records.

## 3. Self-contained setup Q&A (only on consent)

Ask, one at a time:
1. What is this project — one sentence on purpose and users?
2. What are the major components or subsystems?
3. What are the first 1-3 milestones, and what gates each?
4. What is the very first epic to start on?

firetrail does NOT generate docs — `doc add` only *adopts* an existing `.md`
file. So you author the markdown, then adopt + link it.

## 4. Author the docs

Write `docs/ARCHITECTURE.md` (purpose, components, data flow) and
`docs/ROADMAP.md` (milestones + gates) from the answers. Keep them concise
and concrete.

## 5. Adopt + link the docs

```
firetrail doc add docs/ARCHITECTURE.md --type reference --title "Architecture"
firetrail doc add docs/ROADMAP.md      --type reference --title "Roadmap"
```
Link each doc to the work items it informs once they exist (Step 6):
```
firetrail doc link <doc-id> <work-item-id>   # the edge prime follows
```
`doc link` is the ONLY thing that makes `prime --task` deliver the doc — there
is no frontmatter shortcut.

## 6. Seed the first work

```
firetrail epic create "<first milestone epic>"
firetrail task create "<task>" --epic <epic-id>      # repeat for each task
firetrail dep  add <dependent-id> <prereq-id>        # wire EVERY edge
firetrail criteria add <task-id> "<acceptance criterion>"
```
For anything beyond a single task, hand off to `firetrail-epic-breakdown` to
wire and verify the dependency graph.

## 7. Verify

```
firetrail graph <epic-id>   # the shape looks right?
firetrail ready             # only the genuine leaves are unblocked?
```

Then return to the router loop and claim the first ready item.
```

- [ ] **Step 2: Verify frontmatter**

Run: `head -4 crates/ft-cli/templates/skills/firetrail-bootstrap/SKILL.md`
Expected: `---`, `name: firetrail-bootstrap`, `description:` line, `---`.

- [ ] **Step 3: Commit**

```bash
git add crates/ft-cli/templates/skills/firetrail-bootstrap/SKILL.md
git commit -m "feat(ft-cli): add firetrail-bootstrap skill template"
```

---

## Task 3: Epic-breakdown skill — `firetrail-epic-breakdown/SKILL.md`

**Files:**
- Create: `crates/ft-cli/templates/skills/firetrail-epic-breakdown/SKILL.md`

- [ ] **Step 1: Create the epic-breakdown skill file**

Write exactly this content:

```markdown
---
name: firetrail-epic-breakdown
description: Use when breaking a firetrail epic into multiple tasks or planning any multi-task work. Enforces full dependency-graph wiring with dep add, then verification via graph and ready, so parallel claims do not produce work that has to be unwound. A flat list of sibling tasks with no edges is a bug.
---

# Firetrail epic breakdown (dependency wiring)

**If an epic contains more than one task, you MUST wire the dependency graph
before anyone — human or agent — runs `firetrail ready`.** Without edges every
task appears unblocked and parallel claims produce work that has to be unwound
(you can't build `useWeather` before the API module exists). The graph is what
makes `firetrail ready` useful.

## Procedure

1. Create the epic and all task titles first (titles only):
   ```
   firetrail epic create "Ship v1"
   firetrail task create "Build auth"  --epic <epic-id>
   firetrail task create "Login form"  --epic <epic-id>
   ```
2. Map the dependency graph (on paper or in your head): which task cannot
   start until another finishes?
3. Wire EVERY execution edge in **leaf-to-root order** (so you never reference
   an id that doesn't exist yet):
   ```
   firetrail dep add <dependent-id> <prereq-id>   # <dependent> depends on <prereq>
   ```
4. Verify the shape:
   ```
   firetrail graph <epic-id>
   ```
5. Confirm only the genuine leaves are unblocked:
   ```
   firetrail ready
   ```
6. Add acceptance criteria to each task (required before close):
   ```
   firetrail criteria add <task-id> "<criterion>"
   ```

## Two relation surfaces

- **`firetrail dep add <from> <to>`** — execution dependency. Gates
  `firetrail ready`. Use this for "cannot start until X finishes."
- **`firetrail link <from> <to> --type related-to`** — informational relation
  (this task references that finding). Does NOT gate readiness.

## Hard rule

A flat list of siblings under an epic with no edges is almost always a bug. If
the tasks truly are independent, say so explicitly in the epic description so
reviewers know it was a decision, not an oversight.

Run `firetrail <subcommand> --help` if any syntax is uncertain — `--help` is
canonical.
```

- [ ] **Step 2: Verify frontmatter**

Run: `head -4 crates/ft-cli/templates/skills/firetrail-epic-breakdown/SKILL.md`
Expected: `---`, `name: firetrail-epic-breakdown`, `description:` line, `---`.

- [ ] **Step 3: Commit**

```bash
git add crates/ft-cli/templates/skills/firetrail-epic-breakdown/SKILL.md
git commit -m "feat(ft-cli): add firetrail-epic-breakdown skill template"
```

---

## Task 4: Knowledge skill — `firetrail-knowledge/SKILL.md`

**Files:**
- Create: `crates/ft-cli/templates/skills/firetrail-knowledge/SKILL.md`

- [ ] **Step 1: Create the knowledge skill file**

Write exactly this content:

```markdown
---
name: firetrail-knowledge
description: Use when finishing non-trivial work, when you learned something worth keeping, or when handed an existing ADR / RCA / post-mortem / runbook markdown file. Covers memory capture, importing existing markdown, adopting live design docs, promoting quarantined imports, trust progression, and corpus hygiene.
---

# Firetrail knowledge (capture, import, docs, hygiene)

The corpus is what makes future agents effective. Capture *during* the work,
not after. Aim for one finding or decision per non-trivial task.

## 1. New knowledge typed at the prompt

```
firetrail finding  create "Redis OOM under spike traffic"
firetrail incident create "Checkout 500s on 2026-05-12"
firetrail runbook  create "Rotate Stripe webhook secret" --summary "When and how"
firetrail decision create "Use Postgres advisory locks" --context "..." --decision "..."
firetrail gotcha   create "TLS verify off in staging only"
firetrail capture  --kind memory --title "Quick observation"   # opportunistic
```

| Trigger | Record kind |
|---|---|
| A bug whose cause was non-obvious | `finding create` |
| A production incident | `incident create` |
| A reproducible "do these N steps" procedure | `runbook create` |
| An architectural call future agents must honour | `decision create` |
| A subtle trap that will bite the next person | `gotcha create` |

## 2. Handed an EXISTING markdown file? Import it — don't retype

The one-line `create` commands drop the body. Use the importers, which parse
structured fields (root cause, resolution, action items, lessons; context /
decision / consequences; steps / applies-to) and preserve the full body:

```
firetrail import incidents <dir>   # *.md post-mortems / RCAs
firetrail import adrs      <dir>   # *.md ADRs / decisions
firetrail import runbooks  <dir>   # *.md operational runbooks
```
A single file from chat/paste → drop it in a tempdir and point the importer at
the dir. **Memories are immutable** — fields missed at create time cannot be
patched later, so the importer is the only path that captures structured fields.

Imports land **quarantined** (ADR-0014). After review, promote them:
```
firetrail promote-import <id>
```

## 3. Live design docs (.md you keep editing)

Adopt + link so `prime --task` delivers them to the next agent:
```
firetrail doc add <file.md> --type design   # adr | runbook | reference
firetrail doc link <doc-id> <work-item-id>  # the edge prime follows
firetrail doc index [<doc-id>]              # refresh content_hash; no arg = all
```
The `.md` stays the source of truth; the record is a thin pointer.

## 4. Trust progression

```
firetrail memory review  <id>     # draft → reviewed
firetrail memory promote <id>     # reviewed → verified (capability-gated)
```
Don't self-promote to `verified` without `can_promote_verified`
(`firetrail identity show <id>`).

## 5. Corpus hygiene (run opportunistically)

```
firetrail memory stale                       # records past freshness threshold
firetrail similar <id>                        # find near-duplicates
firetrail memory supersede <id> --with <new>  # replace an outdated record
firetrail memory merge <id1> <id2> --into <canonical-id>
firetrail memory deprecate <id>
firetrail memory archive <id>
```

## Reminder

Memory records cannot ride a code PR — open a separate memory-only PR
(ADR-0009). See `firetrail-pr-safety`. Run `firetrail <subcommand> --help`
when syntax is uncertain.
```

- [ ] **Step 2: Verify frontmatter**

Run: `head -4 crates/ft-cli/templates/skills/firetrail-knowledge/SKILL.md`
Expected: `---`, `name: firetrail-knowledge`, `description:` line, `---`.

- [ ] **Step 3: Commit**

```bash
git add crates/ft-cli/templates/skills/firetrail-knowledge/SKILL.md
git commit -m "feat(ft-cli): add firetrail-knowledge skill template"
```

---

## Task 5: PR-safety skill — `firetrail-pr-safety/SKILL.md`

**Files:**
- Create: `crates/ft-cli/templates/skills/firetrail-pr-safety/SKILL.md`

- [ ] **Step 1: Create the pr-safety skill file**

Write exactly this content:

```markdown
---
name: firetrail-pr-safety
description: Use when about to open, finish, or review a pull request in a firetrail repo. Runs the full pre-PR validator suite (check pr, lint, verify, diff, compact) and enforces the memory-only-PR separation rule. Never bypass a failing check.
---

# Firetrail PR safety (pre-PR validation)

Run this before you open or finish a PR. A failing check is the system
preventing real damage — fix the cause, never bypass.

## Procedure

```
firetrail check pr <base-ref> <head-ref>   # full validator suite
firetrail lint memory --fix                # remediation hints (non-mutating)
firetrail verify                           # history-chain integrity (all records)
firetrail diff <base-ref> <head-ref>       # record-aware diff — review the change
firetrail compact --pr <base-ref>..<head-ref>   # PR-time history compaction
```

`check pr` enforces evidence on trust transitions, acceptance-criteria
completion, memory-only-PR rules, and chain integrity.

## Memory-only-PR rule (ADR-0009)

Memory records (finding, incident, runbook, decision, gotcha) **cannot ride in
a code PR.** If your change touches both code and memory records, split it:
open a separate memory-only PR for the records. `check pr` will fail otherwise.

## Hard rules

- Never bypass a failing check with `--force` (close) or `--no-verify` (git
  hooks). Fix the underlying record.
- Never edit `.firetrail/records/**/*.json` by hand — `firetrail verify` will
  catch the broken `state_hash`/`prev_state_hash` chain (ADR-0017).
- Never delete records — use `firetrail memory archive` or
  `firetrail memory supersede --with <new-id>`.

Run `firetrail <subcommand> --help` when syntax is uncertain.
```

- [ ] **Step 2: Verify frontmatter**

Run: `head -4 crates/ft-cli/templates/skills/firetrail-pr-safety/SKILL.md`
Expected: `---`, `name: firetrail-pr-safety`, `description:` line, `---`.

- [ ] **Step 3: Commit**

```bash
git add crates/ft-cli/templates/skills/firetrail-pr-safety/SKILL.md
git commit -m "feat(ft-cli): add firetrail-pr-safety skill template"
```

---

## Task 6: Point AGENTS.md block at the skills

**Files:**
- Modify: `crates/ft-cli/templates/AGENTS_FIRETRAIL_BLOCK.md` (the "Where the design lives" section, near the end)

- [ ] **Step 1: Add the pointer paragraph**

Find this block at the end of the file:

```markdown
### Where the design lives

- `docs/ARCHITECTURE.md` — system overview
```

Insert a new section immediately BEFORE `### Where the design lives`:

```markdown
### Claude Code skills

In Claude Code, situation-specific skills auto-activate to guide deeper
firetrail workflows: `firetrail-bootstrap` (fresh-repo setup),
`firetrail-epic-breakdown` (dependency wiring), `firetrail-knowledge`
(capture / import / docs), and `firetrail-pr-safety` (pre-PR validation).
The `firetrail` router skill is the always-on entry point. Other agents:
this AGENTS.md block remains the canonical driver.

```

- [ ] **Step 2: Verify the insertion**

Run: `grep -n "Claude Code skills" crates/ft-cli/templates/AGENTS_FIRETRAIL_BLOCK.md`
Expected: one match, on a line before "Where the design lives".

- [ ] **Step 3: Commit**

```bash
git add crates/ft-cli/templates/AGENTS_FIRETRAIL_BLOCK.md
git commit -m "feat(ft-cli): point AGENTS block at the new skill suite"
```

---

## Task 7: Refactor `init.rs` to write the skill tree

**Files:**
- Modify: `crates/ft-cli/src/commands/init.rs` (skill-writing block at 425-442; `default_skill_md()` at ~744-747; doc comments at 12, 238, 402-432)
- Delete: `crates/ft-cli/templates/SKILL.md`

- [ ] **Step 1: Write the failing test (multi-skill presence)**

Add to `crates/ft-cli/tests/init_and_doctor.rs`:

```rust
#[test]
fn init_writes_full_skill_suite() {
    let tr = TestRepo::new().unwrap();
    std::fs::remove_dir_all(tr.firetrail_dir()).unwrap();
    let _ = std::fs::remove_file(tr.root().join("AGENTS.md"));
    let _ = std::fs::remove_file(tr.root().join("CLAUDE.md"));

    let out = run_firetrail(tr.root(), &["init", "--json"]);
    assert!(out.success(), "init failed: {}", out.stderr);

    let skills = tr.root().join(".claude/skills");
    for (dir, name) in [
        ("firetrail", "firetrail"),
        ("firetrail-bootstrap", "firetrail-bootstrap"),
        ("firetrail-epic-breakdown", "firetrail-epic-breakdown"),
        ("firetrail-knowledge", "firetrail-knowledge"),
        ("firetrail-pr-safety", "firetrail-pr-safety"),
    ] {
        let body = std::fs::read_to_string(skills.join(dir).join("SKILL.md"))
            .unwrap_or_else(|e| panic!("missing skill {dir}: {e}"));
        assert!(!body.trim().is_empty(), "skill {dir} is empty");
        assert!(
            body.contains(&format!("name: {name}")),
            "skill {dir} missing frontmatter name"
        );
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p ft-cli --test init_and_doctor init_writes_full_skill_suite`
Expected: FAIL — only `firetrail/SKILL.md` is written today, so the
`firetrail-bootstrap` read panics.

- [ ] **Step 3: Replace `default_skill_md()` with a skill table**

In `crates/ft-cli/src/commands/init.rs`, replace the `default_skill_md` function
(currently around lines 744-747):

```rust
fn default_skill_md() -> String {
    let body = include_str!("../../templates/SKILL.md");
    body.to_string()
}
```

with a table of (relative path under `.claude/skills`, embedded body):

```rust
/// The firetrail agent-skill suite, embedded at compile time. Each entry is
/// `(relative path under `.claude/skills`, file body)`. `init` writes every
/// entry verbatim (overwrite-on-reinit), mirroring the old single-SKILL.md
/// behaviour but for the whole suite.
fn skill_templates() -> [(&'static str, &'static str); 5] {
    [
        (
            "firetrail/SKILL.md",
            include_str!("../../templates/skills/firetrail/SKILL.md"),
        ),
        (
            "firetrail-bootstrap/SKILL.md",
            include_str!("../../templates/skills/firetrail-bootstrap/SKILL.md"),
        ),
        (
            "firetrail-epic-breakdown/SKILL.md",
            include_str!("../../templates/skills/firetrail-epic-breakdown/SKILL.md"),
        ),
        (
            "firetrail-knowledge/SKILL.md",
            include_str!("../../templates/skills/firetrail-knowledge/SKILL.md"),
        ),
        (
            "firetrail-pr-safety/SKILL.md",
            include_str!("../../templates/skills/firetrail-pr-safety/SKILL.md"),
        ),
    ]
}
```

- [ ] **Step 4: Replace the single-file write block with a loop**

In `crates/ft-cli/src/commands/init.rs`, replace the skill block (currently
lines 425-442, starting `let skill_dir = ...` through the closing `}` of the
`match`):

```rust
        let skill_dir = ws.root.join(".claude/skills/firetrail");
        std::fs::create_dir_all(&skill_dir).map_err(|e| CliError::internal(COMMAND, e))?;
        // SKILL.md is wholly firetrail-owned (the skill metadata frontmatter
        // mandates a fixed shape), so we overwrite on every init rather
        // than using the marker-based merge.
        let skill_path = skill_dir.join("SKILL.md");
        let new_skill = default_skill_md();
        let label = ".claude/skills/firetrail/SKILL.md";
        match std::fs::read_to_string(&skill_path) {
            Ok(existing) if existing == new_skill => {
                report.preserved.push(label.to_string());
            }
            _ => {
                std::fs::write(&skill_path, new_skill)
                    .map_err(|e| CliError::internal(COMMAND, e))?;
                report.created.push(label.to_string());
            }
        }
```

with:

```rust
        // The skill suite is wholly firetrail-owned (each SKILL.md frontmatter
        // mandates a fixed shape), so we overwrite on every init rather than
        // using the marker-based merge. Each entry lands at
        // `.claude/skills/<relpath>`.
        let skills_root = ws.root.join(".claude/skills");
        for (relpath, body) in skill_templates() {
            let dest = skills_root.join(relpath);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).map_err(|e| CliError::internal(COMMAND, e))?;
            }
            let label = format!(".claude/skills/{relpath}");
            match std::fs::read_to_string(&dest) {
                Ok(existing) if existing == body => {
                    report.preserved.push(label);
                }
                _ => {
                    std::fs::write(&dest, body).map_err(|e| CliError::internal(COMMAND, e))?;
                    report.created.push(label);
                }
            }
        }
```

- [ ] **Step 5: Delete the obsolete template**

Run:
```bash
git rm crates/ft-cli/templates/SKILL.md
```
Expected: file removed. (Its content now lives at
`templates/skills/firetrail/SKILL.md`.)

- [ ] **Step 6: Update stale doc comments**

In `crates/ft-cli/src/commands/init.rs`:
- Line ~12: change `8. Optionally write `AGENTS.md` and `.claude/skills/firetrail/SKILL.md`.`
  to `8. Optionally write `AGENTS.md` and the `.claude/skills/firetrail*` suite.`
- Line ~238 (walkthrough prompt): change
  `"Write AGENTS.md and .claude/skills/firetrail/SKILL.md? (skipped if present)"`
  to `"Write AGENTS.md and the .claude/skills/firetrail* skill suite? (skipped if present)"`
- Line ~402 / ~409 comments: update the "SKILL.md is the Claude Code skill file"
  comment to reference "the firetrail skill suite under `.claude/skills/`".

- [ ] **Step 7: Run the test to verify it passes**

Run: `cargo test -p ft-cli --test init_and_doctor init_writes_full_skill_suite`
Expected: PASS.

- [ ] **Step 8: Confirm the existing skill test still passes**

Run: `cargo test -p ft-cli --test init_and_doctor init_fresh_writes_full_agents_claude_and_skill`
Expected: PASS — the router still lives at
`.claude/skills/firetrail/SKILL.md`, contains `name: firetrail`. NOTE: the old
assertion `skill.contains("firetrail check pr")` will FAIL because the router
no longer contains PR content. Update that assertion in this step to
`assert!(skill.contains("firetrail-pr-safety"), "router should route to pr-safety skill");`

- [ ] **Step 9: Build the whole workspace to confirm no broken `include_str!`**

Run: `cargo build -p ft-cli`
Expected: compiles clean (no missing-file errors from `include_str!`).

- [ ] **Step 10: Commit**

```bash
git add crates/ft-cli/src/commands/init.rs crates/ft-cli/tests/init_and_doctor.rs
git rm --cached crates/ft-cli/templates/SKILL.md 2>/dev/null || true
git commit -m "feat(ft-cli): write the full skill suite from init (was single SKILL.md)"
```

---

## Task 8: `--no-agents` writes no skills

**Files:**
- Test: `crates/ft-cli/tests/init_and_doctor.rs`

- [ ] **Step 1: Write the test**

Add to `crates/ft-cli/tests/init_and_doctor.rs`:

```rust
#[test]
fn init_no_agents_writes_no_skills() {
    let tr = TestRepo::new().unwrap();
    std::fs::remove_dir_all(tr.firetrail_dir()).unwrap();
    let _ = std::fs::remove_file(tr.root().join("AGENTS.md"));
    let _ = std::fs::remove_file(tr.root().join("CLAUDE.md"));

    let out = run_firetrail(tr.root(), &["init", "--json", "--no-agents"]);
    assert!(out.success(), "init failed: {}", out.stderr);

    assert!(
        !tr.root().join(".claude/skills").exists(),
        "--no-agents must not write any skills"
    );
}
```

- [ ] **Step 2: Run the test**

Run: `cargo test -p ft-cli --test init_and_doctor init_no_agents_writes_no_skills`
Expected: PASS (the skill-writing block is already inside `if !resolved.no_agents`).

- [ ] **Step 3: Commit**

```bash
git add crates/ft-cli/tests/init_and_doctor.rs
git commit -m "test(ft-cli): --no-agents writes no skill files"
```

---

## Task 9: Drift-guard — skills reference only real subcommands

**Files:**
- Test: `crates/ft-cli/tests/init_and_doctor.rs`

This test extracts every `firetrail <token>` reference found **inside fenced
code blocks** of the installed skill bodies and asserts each `<token>` is a real
top-level subcommand parsed from `firetrail --help`. Scanning only inside code
fences eliminates prose false-positives (e.g. "firetrail does NOT generate
docs") — command references always live in code blocks. No new dependency.

- [ ] **Step 1: Write the test**

Add to `crates/ft-cli/tests/init_and_doctor.rs`:

```rust
#[test]
fn skills_reference_only_real_subcommands() {
    let tr = TestRepo::new().unwrap();
    std::fs::remove_dir_all(tr.firetrail_dir()).unwrap();
    let _ = std::fs::remove_file(tr.root().join("AGENTS.md"));
    let _ = std::fs::remove_file(tr.root().join("CLAUDE.md"));
    let out = run_firetrail(tr.root(), &["init", "--json"]);
    assert!(out.success(), "init failed: {}", out.stderr);

    // Real top-level subcommands, parsed from the `Commands:` block of help.
    let help = run_firetrail(tr.root(), &["--help"]);
    let mut cmds = std::collections::HashSet::new();
    let mut in_cmds = false;
    for line in help.stdout.lines() {
        if line.trim_start().starts_with("Commands:") {
            in_cmds = true;
            continue;
        }
        if in_cmds {
            if line.trim().is_empty() {
                break;
            }
            // Indented "  name   description" — first token is the command.
            if line.starts_with(' ') {
                if let Some(tok) = line.trim_start().split_whitespace().next() {
                    cmds.insert(tok.to_string());
                }
            }
        }
    }
    assert!(cmds.contains("ready"), "help parse failed; got {cmds:?}");
    assert!(cmds.contains("doc"), "help parse failed; got {cmds:?}");

    // Scan each installed skill — only inside ``` code fences, where real
    // command references live. `firetrail <token>` must name a real subcommand.
    let skills = tr.root().join(".claude/skills");
    for dir in [
        "firetrail",
        "firetrail-bootstrap",
        "firetrail-epic-breakdown",
        "firetrail-knowledge",
        "firetrail-pr-safety",
    ] {
        let body = std::fs::read_to_string(skills.join(dir).join("SKILL.md")).unwrap();
        let mut in_fence = false;
        for line in body.lines() {
            if line.trim_start().starts_with("```") {
                in_fence = !in_fence;
                continue;
            }
            if !in_fence {
                continue;
            }
            let words: Vec<&str> = line.split_whitespace().collect();
            for w in words.windows(2) {
                // Match a bare `firetrail` token (code blocks are unbackticked).
                if w[0] != "firetrail" {
                    continue;
                }
                let next = w[1];
                // Skip flags, placeholders (`<id>`), and the `firetrail-*`
                // skill names (single hyphenated tokens, not "firetrail X").
                if next.starts_with('-') || next.starts_with('<') {
                    continue;
                }
                let tok: String = next
                    .trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '-')
                    .to_string();
                if tok.is_empty() {
                    continue;
                }
                assert!(
                    cmds.contains(&tok),
                    "skill `{dir}` references unknown subcommand `firetrail {tok}` \
                     (line: {line:?})"
                );
            }
        }
    }
}
```

NOTE: this only validates references inside code fences. Inline-backticked
references in prose (e.g. `` `firetrail prime` ``) are not checked — that is
acceptable, since every canonical command sequence in the skills is written in a
fenced block. If the test flags a real subcommand, the help parser missed it;
debug by printing `cmds`.

- [ ] **Step 2: Run the test**

Run: `cargo test -p ft-cli --test init_and_doctor skills_reference_only_real_subcommands`
Expected: PASS. If it fails naming a token, check whether it is a real
subcommand (`firetrail --help`) or a help-parse miss.

- [ ] **Step 3: Commit**

```bash
git add crates/ft-cli/tests/init_and_doctor.rs
git commit -m "test(ft-cli): drift-guard — skills reference only real subcommands"
```

---

## Task 10: Full validation gate

**Files:** none (verification only)

- [ ] **Step 1: Run the ft-cli test suite**

Run: `cargo test -p ft-cli`
Expected: all tests pass (nextest or cargo test).

- [ ] **Step 2: Lint**

Run: `cargo clippy -p ft-cli --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 3: Format check**

Run: `cargo fmt --all --check`
Expected: clean.

- [ ] **Step 4: End-to-end smoke (manual)**

Run:
```bash
cargo build -p ft-cli
cd "$(mktemp -d)" && git init -q && "$OLDPWD/target/debug/firetrail" init --json >/dev/null
ls .claude/skills
```
Expected: lists `firetrail  firetrail-bootstrap  firetrail-epic-breakdown  firetrail-knowledge  firetrail-pr-safety`.

- [ ] **Step 5: Close the beads epic via commit**

The post-commit hook auto-closes referenced issues. Final commit:
```bash
git commit --allow-empty -m "chore(ft-cli): skill suite complete

Closes: firetrail-uz0x"
```
```

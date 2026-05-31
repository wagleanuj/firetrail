# Firetrail agent-skill suite — design

**Date:** 2026-05-30
**Status:** Approved (brainstorm), pending implementation plan
**Author:** Anuj Wagle (with Claude)

## Problem

`firetrail init` installs exactly one agent-facing skill today —
`.claude/skills/firetrail/SKILL.md`, a flat command reference — plus the
`AGENTS_FIRETRAIL_BLOCK.md` driver injected into `AGENTS.md`. Both are
*reference documents*, not *trigger-activated procedural playbooks*.

Consequences:

- **No fresh-repo entry point.** When an agent lands in an empty firetrail
  workspace (no roadmap/architecture docs, `firetrail ready` returns
  nothing), nothing tells it to offer the user a setup session. It runs the
  discovery loop, finds nothing, and stalls. There is no
  bootstrapping/brainstorming branch anywhere in the installed instructions.
- **The richest features are listed but not operationalized.** Doc authoring
  + linking, the import/promote-quarantine flow, memory hygiene + trust
  promotion, and the recall→capture habit are documented as a wall of
  reference text rather than as situation-triggered workflows the agent is
  pulled into at the right moment.
- **All-or-nothing context.** One broad skill means the entire reference doc
  loads whenever firetrail is touched, even for a single `ready` call, and it
  cannot proactively surface the right deep workflow.

Firetrail's value is a **lifecycle** — bootstrap → plan → recall → build →
capture → review → PR → maintain. This design turns the single reference doc
into a small suite of focused, trigger-activated workflow skills that pull the
agent into the right firetrail commands at the right moment.

## Goal & scope

- **Audience:** *other* repos. These are generic distribution artifacts that
  ship via `firetrail init` so any team using firetrail gets agents that use it
  well. They must be project-agnostic and self-contained (no dependency on
  superpowers or any other plugin, since target repos may not have them).
- **Out of scope:** dogfooding-only skills, web-UI skills, identity/scope and
  search-infra/daemon skills (kept as router gotchas — rarely the agent's job).

## Architecture

### File layout

New template tree `crates/ft-cli/templates/skills/`, embedded in the binary and
copied by `firetrail init` into the target repo's `.claude/skills/`:

```
.claude/skills/
  firetrail/SKILL.md                 # router (always-on entry; replaces today's SKILL.md)
  firetrail-bootstrap/SKILL.md       # A — fresh-repo setup
  firetrail-epic-breakdown/SKILL.md  # B — dependency wiring
  firetrail-knowledge/SKILL.md       # C — capture / import / docs / corpus hygiene
  firetrail-pr-safety/SKILL.md       # D — pre-PR validation
```

The structure is **hybrid**: one thin always-on router + four deep skills that
load only when their situation fires.

### Content division (the no-duplication rule)

Three layers, each owning distinct content so nothing is repeated:

| Layer | Audience | Trigger-gated? | Owns |
|---|---|---|---|
| `AGENTS.md` block (`AGENTS_FIRETRAIL_BLOCK.md`) | All agents (cross-tool) | No — always read | The non-negotiables: mandatory loop summary, hard rules (never hand-edit records, never `--no-verify`, recall-before-build, capture-after-build, memory-only-PR), where the design lives, identity. Stays comprehensive because non-Claude agents rely on it. |
| Router skill `firetrail` | Claude Code | Yes (broad: any firetrail activity) | Lean: loop in one block, "five things agents get wrong," and the **dispatch table** (situation → deep skill). Its only unique job is surfacing the deep skills. |
| Deep skills A–D | Claude Code | Yes (narrow, per-situation) | The heavy step-by-step playbooks. New procedural depth that exists nowhere today; `AGENTS.md` only summarizes these flows. |

`AGENTS_FIRETRAIL_BLOCK.md` gains exactly one new line noting that, in Claude
Code, situation-specific skills auto-activate to guide deeper workflows. No
procedural content is duplicated between layers.

## The five skills

### Router — `firetrail`

- **Trigger (`description`):** any firetrail / work-graph / memory / PR activity
  in the repo (broad, so it is the always-available entry point).
- **Body:** mandatory loop (terse) + hard rules (terse) + the dispatch table:

  | If you're… | Use skill |
  |---|---|
  | in a fresh/empty workspace | `firetrail-bootstrap` |
  | breaking an epic into 2+ tasks | `firetrail-epic-breakdown` |
  | done with work / capturing knowledge / handed an ADR | `firetrail-knowledge` |
  | about to open or finish a PR | `firetrail-pr-safety` |

### A — `firetrail-bootstrap`

- **Trigger:** "starting in a firetrail repo with no roadmap/architecture docs
  and no open work."
- **Detection (conservative):** run `firetrail doctor` and `firetrail ready`;
  check for `docs/ROADMAP.md` and `docs/ARCHITECTURE.md`. Treat the workspace as
  "fresh" only when there are no open/active work records *and* no roadmap doc.
  This avoids false-firing on a mature repo whose `ready` is momentarily empty.
- **Consent gate:** when fresh, **ASK the user** whether they want to set up the
  project before doing anything. Never auto-generate.
- **Flow on consent:** self-contained Q&A (project purpose, major components,
  milestones — no dependency on the superpowers brainstorming skill) → write
  `docs/ARCHITECTURE.md` + `docs/ROADMAP.md` → adopt them with
  `firetrail doc add <file> --type design|reference` and
  `firetrail doc link <doc-id> <work-item-id>` → seed the first epic + tasks +
  `firetrail dep add` edges → verify with `firetrail graph <epic>` and
  `firetrail ready`.
- **Note on doc generation:** firetrail does not generate docs; `doc add` only
  *adopts* an existing `.md` file. The skill therefore guides the agent to
  author the `.md` files (with the user), then adopt + link them.

### B — `firetrail-epic-breakdown`

- **Trigger:** "breaking an epic into multiple tasks / planning multi-task
  work."
- **Flow:** create the epic and all task titles first → map the dependency graph
  → `firetrail dep add <from> <to>` each edge in leaf-to-root order →
  `firetrail graph <epic-id>` to verify shape → `firetrail ready` to confirm
  only genuine leaves are unblocked → add acceptance criteria.
- **Hard rule:** a flat list of sibling tasks under an epic with no dependency
  edges is almost always a bug; if the tasks truly are independent, say so
  explicitly in the epic description.

### C — `firetrail-knowledge`

- **Trigger:** "finished non-trivial work, learned something, or handed an
  existing ADR / RCA / runbook `.md`."
- **Flow:**
  - New knowledge typed at the prompt → pick the kind and `create`
    (`finding` / `decision` / `gotcha` / `incident` / `runbook`), or
    `firetrail capture` for a quick opportunistic note.
  - Existing markdown handed over → `firetrail import incidents|adrs|runbooks
    <dir>` (a single file → drop it in a tempdir first; the importer expects a
    directory and is the only path that captures structured fields, since
    memories are immutable).
  - Live design docs → adopt with `firetrail doc add/link/index`.
  - Quarantined imports → `firetrail promote-import` after review (ADR-0014).
  - Trust progression → `firetrail memory review` (draft→reviewed),
    `firetrail memory promote` (reviewed→verified, capability-gated).
  - **Corpus hygiene (folded in):** `firetrail memory stale`, `supersede`,
    `merge`, and dedup via `firetrail similar`.
- **Reminder:** memory records cannot ride a code PR; open a separate
  memory-only PR (ADR-0009).

### D — `firetrail-pr-safety`

- **Trigger:** "about to open, finish, or review a PR."
- **Flow:** `firetrail check pr <base>..<head>` → `firetrail lint memory --fix`
  → `firetrail verify` → `firetrail diff <base>..<head>` →
  `firetrail compact --pr <base>..<head>` → enforce memory-only-PR separation.
- **Hard rule:** never bypass a failing check with `--force` or `--no-verify`;
  fix the cause.

## Install mechanics (`crates/ft-cli/src/commands/init.rs`)

- Replace the current single-file copy (`templates/SKILL.md` →
  `.claude/skills/firetrail/SKILL.md`) with a **recursive copy** of the embedded
  `templates/skills/` tree into `.claude/skills/`.
- Preserve existing semantics:
  - Skill files are managed artifacts — overwrite them on every `init` (same as
    `SKILL.md` today).
  - Still gated by `--no-agents`.
  - `AGENTS.md` user content outside the `<!-- firetrail:begin -->` /
    `<!-- firetrail:end -->` managed block is still preserved on re-init.
- Templates remain embedded in the binary the same way the current ones are.

## Testing

- **init smoke test:** after `firetrail init`, assert all five skill files exist
  under `.claude/skills/`, are non-empty, and have valid frontmatter with
  `name` and `description`.
- **`--no-agents` test:** assert none of the skill files are written.
- **Drift guard (in scope):** a test that extracts every `firetrail
  <subcommand>` token referenced in the skill bodies and asserts each is a real
  CLI subcommand, catching the "templates drift from the CLI" problem the repo
  already worries about. Scoped to the top-level subcommand (e.g. `doc`,
  `import`, `memory`) to keep the matcher simple and robust.

## Edge cases & risks

- **Bootstrap false-firing on a mature repo:** mitigated by conservative
  detection (no work records *and* no roadmap doc) plus an unconditional consent
  gate before any action.
- **Target repo without superpowers/brainstorming:** every skill is
  self-contained; bootstrap carries its own Q&A rather than delegating.
- **Non-Claude agents:** skills are Claude-Code-specific (`.claude/skills/`);
  those agents still get the full `AGENTS.md` block, which remains the
  cross-tool canonical driver.
- **Re-init idempotency:** skill files overwritten; user-authored AGENTS.md
  content preserved outside the managed block.
- **Over-triggering / overlap between deep skills:** descriptions are written to
  be mutually exclusive by situation (fresh-repo vs. epic-breakdown vs.
  post-work capture vs. PR); the router dispatch table is the disambiguator.

## Non-goals

- No web-UI, identity/scope, or daemon/search-infra skills.
- No change to the CLI surface itself — this is purely the agent-facing
  instruction layer plus the `init` copy mechanics.
- No dependency on any external skill ecosystem.

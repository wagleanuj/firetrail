---
name: firetrail-bootstrap
description: Use when starting in a firetrail repo that has no roadmap or architecture docs and no open work — a fresh or empty workspace — or when `firetrail doctor` reports a missing or unconfirmed repo profile. Asks the user for consent, then guides a self-contained setup session that drafts ARCHITECTURE.md and ROADMAP.md, adopts them as Doc records, seeds the first epic and tasks with dependency edges, and populates the repo profile (validate/test/build/lint commands, tooling facts, component map) by inspecting the repo and confirming with the user. In a monorepo it also sets per-scope profile deltas so each package gets its own validate/test commands.
---

# Firetrail bootstrap (fresh-repo setup)

Use this when you land in a firetrail workspace that looks empty. Do NOT
auto-generate anything — detect, then ASK the user before acting.

Steps 1–7 set up a fresh workspace (docs + first work). **Step 8 (repo
profile)** is semi-independent: if `firetrail doctor` reports a missing or
unconfirmed profile in a repo that already has work, jump straight to Step 8 —
the rest of this skill doesn't apply, but the profile flow still does.

## 1. Detect (conservative)

```
firetrail doctor          # workspace healthy?
firetrail ready           # any unblocked work?
firetrail list            # any work records at all?
```
Also check for `docs/ROADMAP.md` and `docs/ARCHITECTURE.md`.

Treat the workspace as **fresh** only when there are no open/active work
records AND no roadmap doc. If the repo already has work or a roadmap, the
fresh-setup steps (2–7) do not apply — return to the router and use `firetrail
ready`. (Exception: if `doctor` flagged the repo profile, still do Step 8.)

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

## 8. Populate the repo profile

The **repo profile** is a record of repo facts every other firetrail tool reads
from: the canonical validate/test/build/lint commands, the language/tooling
facts, and a shallow component map. `firetrail doctor` warns when it is missing
or still unconfirmed — that warning is what sends you here, whether or not the
rest of this skill applied.

In a **monorepo** the profile is a repo-wide **base** plus optional **per-scope
deltas** — one package can override just its `test` command while inheriting
everything else from the base. A standalone repo has only the base and never
touches scopes (zero overhead). Steps 8a–8e set up the base; **Step 8f** adds
per-scope deltas when the repo warrants it.

firetrail ships **no detection heuristics** — this is YOUR judgment (ADR-0005).
firetrail only stores, indexes, and surfaces what you decide. So:

**8a. Inspect.** Read the repo to discover the *real* commands and layout — do
not guess:
- Manifests: `Cargo.toml`, `package.json` (its `scripts`), `Makefile`,
  `justfile`, `pyproject.toml`, `go.mod`, etc.
- CI configs: `.github/workflows/*.yml`, and any other pipeline files — these
  usually spell out the exact validate/test/lint invocations the team trusts.
- Directory layout (e.g. `crates/`, `packages/`, `apps/`, `src/`) for a
  **shallow** component map — names + paths only, not deep docs.

**8b. Propose & discuss.** Tell the user what you found and confirm it, one piece
at a time. Take **extra care with the validate command** — that is the single
"prove a change is good" command the audit loop will run, so it must be the one
the user actually trusts as the gate. Formatting belongs *inside* validate or
lint (e.g. `cargo fmt --check && cargo test && cargo clippy`); there is no
separate format field.

**8c. Persist incrementally.** As each piece is confirmed, write it. `profile
set` is a partial update — only the flags you pass change; everything else is
preserved — so call it repeatedly as the discussion unfolds. It creates the
record if absent, updates in place if present, and always writes it as `Draft`
with `origin = Agent` (your proposal, not yet confirmed):

```
firetrail profile set --validate "<cmd>" --test "<cmd>" --build "<cmd>" --lint "<cmd>" \
                      --language rust --language typescript \
                      --package-manager cargo --package-manager pnpm \
                      --runtime "node 20" --note "<free text>"

firetrail profile component add <name> <path> --summary "<one line>"   # repeat per component
firetrail profile component rm  <name>                                 # if you got one wrong

firetrail profile show          # review what's stored (add --json for machine output)
```
`--language` and `--package-manager` are repeatable; passing any value
overwrites the stored list, so include the full set each time you set them.

**8d. Finalize.** Once the user is happy, confirm the profile by transitioning it
out of `Draft` via the existing trust commands — that, not the write itself, is
the signal downstream tools trust:

```
firetrail memory review  <profile-id> --reason "<why confirmed>"   # Draft → Reviewed
firetrail memory promote <profile-id>                              # Reviewed → Verified (optional)
```
Get `<profile-id>` from `firetrail profile show`.

**8e. New / sparse repos.** When there's little to detect, mostly *ask* the user.
Leaving `validate_command` empty initially is fine — `firetrail doctor` keeps
nudging until it's filled and confirmed; don't invent a command just to silence
it. Re-running this step later (the build setup changed, a component moved) is
just another `profile set` round: it updates the record in place and re-enters
`Draft` until re-confirmed.

> `firetrail doctor --strict` (for CI) exits non-zero when the validate command
> is empty or the profile is still unconfirmed — so getting through 8a–8d
> unblocks a `--strict` pipeline.

## 8f. Monorepos — per-scope profiles

Do this **only** when the repo is a monorepo whose packages need *different*
validate/test/build commands (e.g. a Rust crate validated with `cargo` next to
a JS app validated with `pnpm`). If one repo-wide validate command covers
everything, the base profile from 8a–8e is enough — skip this step.

**Detect a monorepo.** Signals: a workspace manifest (`[workspace]` in
`Cargo.toml`, `pnpm-workspace.yaml`, `nx.json`, `turbo.json`, `go.work`) or
several `package.json` / manifests under `apps/`, `packages/`, `crates/`. This
is YOUR judgment, not a firetrail heuristic (ADR-0005).

**Per-scope profiles require scopes.** A per-scope delta is keyed to a scope id,
and `firetrail profile set --scope <id>` **errors if `<id>` is not a declared
scope**. Check what exists:

```
firetrail scope list      # the scopes declared in .firetrail/scopes.yaml
```

If the packages aren't declared yet, author `.firetrail/scopes.yaml` first (it's
hand-edited — there is no `scope add` command). Declare **broad patterns first,
narrow exceptions last**: resolution is **last-declared-wins** (the same rule as
CODEOWNERS), so a catch-all belongs at the *top*.

```yaml
# .firetrail/scopes.yaml
scopes:
  - id: apps/checkout
    applies_to: ["apps/checkout/**"]
  - id: libs/ui
    applies_to: ["libs/ui/**"]
```

**Set the base to the common case, then override only the deltas.** A scope
profile is a *sparse* delta: it sets only what differs; every unset field
inherits the base. Don't re-state shared commands per scope.

```
# base = the repo-wide default (8a–8e already did this)
firetrail profile set --validate "just ci" --test "cargo test"

# per-package overrides — only the field that differs
firetrail profile set --scope apps/checkout --test "pnpm --filter checkout test"
firetrail profile set --scope libs/ui       --validate "pnpm --filter ui lint && pnpm --filter ui test"
```

**Verify the resolution.** Confirm each scope resolves to the command you
expect, and that a changeset maps to the right distinct commands:

```
firetrail profile list                          # base + every scope delta, one row each
firetrail profile show --scope apps/checkout --resolved   # the merged base ⊕ delta
firetrail profile resolve --staged              # distinct validate commands for the staged diff
```

**Confirm each delta's trust.** Like the base, every scope delta is written as
`Draft`; confirm the ones the user trusts via the trust commands (8d), using its
own id from `firetrail profile list`.

> Each scope's validate command feeds `firetrail doctor --strict` and the audit
> loop independently — so a scope with no resolved validate command (its own or
> inherited from base) is a `--strict` gap, surfaced by `doctor`.

Then return to the router loop and claim the first ready item.

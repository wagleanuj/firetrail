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

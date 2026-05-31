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

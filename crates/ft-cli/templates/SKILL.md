---
name: firetrail
description: Use the `firetrail` CLI for work-graph queries, dependency wiring, memory capture, recall, and PR validation in this repo. Read AGENTS.md for the comprehensive driver — this is the quick-reference companion.
---

# Firetrail skill

The repo uses `firetrail` for task tracking, dependency graphs, incident
memory, search/recall, and PR safety. **`AGENTS.md` at the repo root is
the canonical driver — read it before doing real work.** This skill is
the quick reference.

## The five things agents most often get wrong

1. **Creating a flat list of sibling tasks under an epic with no
   dependency edges.** If an epic has 2+ tasks, you MUST wire
   `firetrail dep add <from> <to>` for every execution dependency
   before anyone runs `firetrail ready`. Without edges, every task
   appears unblocked and parallel claims will produce work that has
   to be unwound.
2. **Claiming work without recall first.** Always run
   `firetrail search "<keywords>"` and `firetrail prime --task <id>`
   *before* `firetrail claim`. The corpus exists so future agents
   don't re-solve solved problems.
3. **Finishing work without capturing what was learned.** Every
   non-trivial task should produce at least one finding, decision, or
   gotcha. The corpus grows or it dies.
4. **Guessing CLI syntax from memory.** Run
   `firetrail <subcommand> --help` first. Templates drift; `--help`
   is canonical.
5. **Editing `.firetrail/records/**/*.json` by hand or bypassing git
   hooks with `--no-verify`.** The hash chain catches tampering. Use
   the CLI exclusively.

## Decision flow

1. Need work?
   ```
   firetrail ready
   firetrail show <id>
   firetrail search "<task keywords>"       # has this been solved before?
   firetrail prime --task <id>              # context pack
   firetrail claim <id>
   ```

2. Breaking an epic into tasks?
   ```
   firetrail epic create "..."
   firetrail task create "..." --epic <epic-id>      # repeat
   firetrail dep  add <dependent> <prereq>           # wire EVERY edge
   firetrail graph <epic-id>                         # visually verify
   firetrail ready                                   # only leaves unblocked?
   ```

3. Discovered something worth keeping?
   ```
   firetrail finding   create "<summary>"
   firetrail incident  create "<summary>"
   firetrail decision  create "<title>" --context "..." --decision "..."
   firetrail runbook   create "<title>"  --summary "..."
   firetrail gotcha    create "<summary>"
   firetrail capture   --kind memory --title "..."   # opportunistic
   ```

4. Finished work?
   ```
   firetrail close <id>                              # AC must be complete
   ```

5. Opening a PR?
   ```
   firetrail check pr <base-ref> <head-ref>
   firetrail lint memory --fix
   firetrail verify
   ```

## Recall before build, capture after build

```
search   → look across the whole corpus by keyword (lexical + vector)
similar  → nearest records to a given id
prime    → assemble a context pack (token-budgeted) for a task or query
finding  → "I learned this and the next person needs it"
decision → "I chose X over Y for these reasons"
gotcha   → "this looks fine but bites you"
```

## Authoritative help

```
firetrail --help                 # full subcommand surface
firetrail <subcommand> --help    # canonical syntax (templates can drift)
firetrail doctor                 # workspace health
```

## Common gotchas

- Default builds use the mock embedder. Semantic search needs
  `--features ft-embed/onnx` at build time; otherwise lexical search
  fully covers `firetrail search`.
- The daemon socket lives under `~/.cache/firetrail/<repo-hash>/`, not
  in the repo. `firetrail daemon start` spawns it as needed.
- Memory records (finding, incident, runbook, decision, gotcha) cannot
  ride in a code PR — open a separate memory-only PR (ADR-0009).
- Concurrent claims on the same record exit with code 3 (Conflict).
  That's correct behaviour — coordinate or `firetrail claim-takeover`.
- Trust progression is `draft → reviewed → verified`. You cannot
  self-promote to `verified` without the `can_promote_verified`
  capability. Check via `firetrail identity show <id>`.

See `AGENTS.md` for the comprehensive workflow, the full list of
gotchas, and design references.

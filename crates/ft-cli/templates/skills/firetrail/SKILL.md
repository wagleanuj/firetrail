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

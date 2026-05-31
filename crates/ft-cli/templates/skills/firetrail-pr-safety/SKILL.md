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

---
name: firetrail
description: Use the `firetrail` CLI for work-graph queries, memory capture, search, and PR validation in this repo. Read AGENTS.md first.
---

# Firetrail skill

The repo uses `firetrail` for task tracking, incident memory, search, and
PR safety. The agent-facing driver lives in `AGENTS.md` at the repo root
— read it before issuing any command. This skill is a quick-reference
companion.

## Decision flow

1. Need work? → `firetrail ready` then `firetrail show <id>` then `firetrail claim <id>`.
2. Finished work? → `firetrail close <id>` (acceptance criteria must be complete).
3. Found something worth remembering? → `firetrail finding|incident|decision|runbook|gotcha …`.
4. Need context before touching code? → `firetrail prime --task <id>` or `firetrail prime --query "…"`.
5. About to open a PR? → `firetrail check pr`.

## Authoritative help

```
firetrail --help
firetrail <subcommand> --help
firetrail doctor          # verify workspace health
```

## Hard rules

- Never edit `.firetrail/records/**/*.json` by hand — the hash chain
  catches tampering.
- Never delete records — `memory archive`, `supersede`, or `redact`.
- Never bypass git hooks with `--no-verify` — they enforce chain
  integrity; fix the underlying record instead.
- `--force --reason "…"` is auditable, not a shortcut.

## Common gotchas

- Default builds use the mock embedder. Semantic search needs
  `--features ft-embed/onnx` at build time; otherwise lexical search
  fully covers `firetrail search`.
- The daemon socket lives under `~/.cache/firetrail/<repo-hash>/`, not
  in the repo.
- Memory records (finding, incident, runbook, decision, gotcha) cannot
  ride in a code PR — open a separate memory-only PR (ADR-0009).
- Concurrent claims on the same record exit with code 3 (Conflict),
  which is correct behavior; coordinate or `claim-takeover`.

See `AGENTS.md` for the comprehensive workflow.

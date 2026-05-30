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
   firetrail prime --task <id>              # context pack (incl. linked docs)
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

   Have an existing post-mortem doc, RCA, ADR, or runbook in markdown?
   Do NOT retype it into `... create "..."` — the one-line `create`
   commands drop the body. Use the importers instead:
   ```
   firetrail import incidents <dir>   # *.md post-mortems / RCAs
   firetrail import adrs      <dir>   # *.md ADRs / decisions
   firetrail import runbooks  <dir>   # *.md operational runbooks
   ```
   The importers parse title, summary, root cause, resolution, action
   items, and lessons learned (incidents); context, decision, and
   consequences (ADRs); steps and applies-to (runbooks). They preserve
   the full markdown body and report a `parse_confidence` per file.

   Single file from chat / paste / a non-standard location? Drop it
   into a temp dir and point the importer at it; that's the only path
   that captures structured fields. Memories are immutable — fields
   missed at create time cannot be patched in later.

   Produced a design / ADR / runbook / reference doc as a live `.md`
   file? Adopt it as a Doc record and link it so the next agent gets it
   on `prime`. The `.md` file stays the source of truth — the record is
   a thin pointer with a `content_hash`.
   ```
   firetrail doc add <file.md> --type design   # adr | runbook | reference
   firetrail doc link <doc-id> <work-item-id>  # the link prime follows
   firetrail doc index [<doc-id>]              # refresh after editing the file
   ```
   `doc link` is the ONLY thing that makes `prime --task` deliver the
   doc — there is no frontmatter shortcut. Linking is safe after edits:
   a stale `content_hash` is re-indexed lazily when `prime` reads the
   doc, so run `doc index` (no arg = all docs) only to refresh eagerly.

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
- A Doc record points at a live `.md` file — it does not copy the body.
  Move or rename the file and the pointer dangles (the doc shows as a
  broken link). Restore the path, or `doc add` the new path and
  `doc link` it again — re-adding mints a new record id, so the old
  link won't follow. `doc link` is what makes `prime --task` deliver
  the doc.

See `AGENTS.md` for the comprehensive workflow, the full list of
gotchas, and design references.

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

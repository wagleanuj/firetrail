# Firetrail docs — convention

This is how plans, architecture, design docs, ADRs, and runbooks are linked to
work in firetrail so a fresh agent (or teammate) picking up a ticket gets the
right, current docs. Full design rationale:
`docs/superpowers/specs/2026-05-29-firetrail-docs-design.md`.

## The model

The `.md` file is the **single source of truth** for content. Firetrail stores a
thin `Doc` record that *points at* the file — it never holds a second copy of
the prose. The record carries the file `path`, a `content_hash` (drift
detection), a short `summary`, an open `doc_type` tag, `trust`, and relations to
work items. Search/`prime` read the file; editing the file is just editing
markdown (surgical, PR-reviewable).

## Frontmatter

Put a YAML frontmatter block at the top of each doc. `firetrail doc index` reads
it verbatim:

```yaml
---
doc_type: design        # open tag — conventional: design | adr | runbook | reference
status: draft           # maps to trust: draft | reviewed | verified | ...
links:                  # work items this doc documents (epic/task ids)
  - firetrail-2mwp
scope: ft-ui            # optional owning scope
---
```

- `doc_type` is an **open** tag. The conventional values cover most needs, but
  teams may use custom ones — firetrail does not enforce a taxonomy.
- `links` is what makes the doc reachable from a ticket. It creates a
  `DocumentedIn` relation (`task/epic → doc`); `prime --task <id>` then delivers
  the linked doc (summary + path) into the agent's context pack.

## Where docs live

`docs/` keeps its existing shape — no reorganization:

| Directory | Purpose |
|---|---|
| `ARCHITECTURE.md` | System integration view |
| `ROADMAP.md` | Milestones and gates |
| `decisions/` | ADRs (numbered) |
| `components/` | Per-crate specs |
| `plans/` | Implementation plans |
| `superpowers/specs/` | Design specs |

Adding frontmatter to a file is all it takes to make it a first-class,
linkable, searchable `Doc`.

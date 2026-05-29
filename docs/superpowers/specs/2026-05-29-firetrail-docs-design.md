# Firetrail Docs — file-backed design/architecture docs linked to work

**Date:** 2026-05-29
**Status:** Design approved (brainstorm complete); awaiting plan.
**Scope:** Phase 1 (convention) + Phase 2 (data/prime wiring) + Phase 3 (ticket docs panel).
Later surfaces (library/freshness view, graph coverage, prime-preview) noted as follow-on.

---

## 1. Problem

While building a feature, a team produces prose that explains *what* to build, *why*,
and *how*: design docs, architecture notes, ADRs, runbooks, roadmaps. Today that prose
either rots in a wiki (Confluence/Notion) linked to a ticket by a bare hyperlink that's
stale within a quarter, or — in this repo — lives as hand-written markdown in `docs/`
referenced from beads tickets as **plain-text paths** that an agent must grep to find.

The pain is not categorization; it is **linking, freshness, and discovery**. A fresh
agent (or teammate) who picks up a ticket cannot reliably get the right, current docs
for that work.

Firetrail is purpose-built for exactly this — "tasks, dependencies, findings, runbooks,
and decisions stored as JSON in git, searchable locally, **primed into agent context**."
The goal of this design is to make plans/architecture/design docs **first-class,
linked to work items, and surfaced by `prime`**, so picking up a ticket delivers the
docs needed to solve it.

## 2. Goal & approach

Extend firetrail so a long-form doc is a first-class, linkable, searchable, trust-tracked
entity — without sacrificing the authoring/review ergonomics of plain markdown.

Sequenced as **convention now, product after** (every step ships standalone value,
nothing is blocked on the UI):

1. **Convention** — a `docs/` layout + frontmatter that links docs to work items. Zero
   code; usable today; it is the input format the tooling later consumes.
2. **Data + prime wiring** — a file-backed `Doc` record, writable doc↔work relations,
   `content_hash` freshness, and `prime` surfacing linked docs. Value lands for *agents*
   via the CLI before any UI exists.
3. **UI: docs-on-ticket panel** — the human-facing mirror of what `prime` already
   delivers to agents.

## 3. Key decisions

### 3.1 File-backed pointer model (not markdown-in-JSON)

The `.md` file is the **single source of truth**. Firetrail stores a *thin* `Doc`
record that *points at* the file — it never holds a second copy of the prose.

Rationale: an agent has two opposite relationships with a doc. **Authoring/editing**
wants a plain file (surgical `Edit`/`Grep`, readable PR diffs); prose locked in a JSON
string field is painful to edit and unreadable in review. **Retrieval** wants the DB
(typed links, semantic search, trust ranking, token-budgeted `prime`). The answer is
files for content, DB for the index/graph over that content — which is exactly
firetrail's stated two-substrate model (*git is truth, SQLite is derived cache*).

This is a deliberate **departure** from today's model, where memory kinds
(`Finding`/`Decision`/`Memory`/…) store markdown as embedded JSON strings. Existing
native kinds are left as-is for now (see Non-goals).

The `Doc` record holds:

- `path` — repo-relative path to the `.md` file (git-native).
- `content_hash` — hash of the file's content at index time (drift detection).
- `title`, `summary` — short excerpt for list/prime rendering (derived from the file).
- `doc_type` — **open** string tag (see 3.2).
- `trust` (`TrustState`), `owning_scope`/`affected_scopes` — as other records.
- relations to work items (see 3.3).

The embedding and FTS text are computed from the **file contents**, not a JSON body
(see 3.4). `Evidence.content_hash` exists today but is unused; `Doc` gets its own
`content_hash` field for this purpose.

### 3.2 One `Doc` kind, open `doc_type` tag (no enforced taxonomy)

A single new `RecordKind::Doc`. The taxonomy lives in an **open** `doc_type` tag, not
the type system. Conventional values: `design`, `adr`, `runbook`, `reference`.

Rationale (grounded in how large teams actually work): docs-as-code, RFC/design-doc
culture, ADRs, and Backstage's catalog+TechDocs all validate "file-backed entity +
typed relations." But real orgs use **few, loose** doc types and a single freeform
"design doc" that covers what/why/how together — *not* a rigid Spec/Plan/Roadmap
hierarchy. Forcing authors to pick among many categories creates friction and
miscategorization. The value is in the link + freshness + retrieval, not the taxonomy.

Adding one kind touches storage/builder/enums/UI once; six kinds multiply that cost for
no real benefit (YAGNI).

### 3.3 Linking via relations that already exist

Work↔doc links use the `RelationKind` variants already declared (but currently
forward-compat only): `DocumentedIn` (`Task`/`Epic` → `Doc`) and its inverse
`ImplementedBy` (`Doc` → `Task`/`Epic`).

Changes required:
- Add `DocumentedIn`/`ImplementedBy` to the **writable** relation subset.
- Surface them in the **index** (today the index only auto-derives Task↔Epic and
  Subtask↔Task edges; relation edges from `relations.jsonl` are already ingested, so
  this is mainly about ensuring doc edges are walked).
- Walk them in **`prime`** so a linked `Doc` is included as a *required, never-truncated*
  item — delivered as **link + summary + path**, with the agent reading the full file on
  demand. (Inlining a 2,000-line architecture doc would blow the 8k-token budget;
  delivering the pointer keeps the pack small and lets the agent pull detail selectively.)

### 3.4 Semantic search unaffected

Embedding is computed from `ft_embed::record_text(record)` (extraction over text), not
"a JSON field." For `Doc`, extraction reads the linked `.md` file and returns its text.
FTS5 indexes the same. So a `Doc` gets the full hybrid treatment (BM25 + 384-dim vector,
trust/recency-weighted) identically to a `Finding` — `prime`/`search` need not know the
content came from a file.

### 3.5 Freshness: lazy `content_hash` is the correctness backbone

Today, records re-index **synchronously on every write through ops** (rewrite JSON →
refresh relational index → upsert FTS row → dispatch text to embed daemon → upsert
vector; the daemon's BLAKE3(text) cache means unchanged text is a cache hit). The embed
daemon is a **pull-driven RPC compute service, not a filesystem watcher**, and it
self-terminates after 300s idle; if it's down, FTS still updates but the **vector is
silently skipped**. Critically, **out-of-band edits** (editing a file directly, a PR
merge) **re-index nothing** — `search` does no staleness check and silently serves stale
results until a manual `firetrail index refresh`.

The pointer model's whole premise is files edited directly, so drift detection becomes
**load-bearing**, not optional. Architecture:

- **Correctness mechanism (load-bearing):** a **lazy `content_hash` check** in
  `prime`/`search`. Before serving, compare each linked file's current hash to the
  record's stored `content_hash`; on mismatch, re-index that doc on demand (re-hash,
  refresh FTS, re-embed if the daemon is up, refresh the summary excerpt). If the file is
  missing, surface a **broken-link** state rather than a silent omission. This runs at the
  exact moment freshness matters (read time) and is cross-platform with no always-on
  process.
- **Eager warmers (latency optimizations, not correctness):**
  - Edits *through* firetrail (ft-ui editor, `firetrail doc` CLI) already re-index
    synchronously via the ops path — no extra work.
  - A **git-hook warmer** at commit (firetrail already installs git hooks; this repo
    already runs a beads post-commit hook) runs `firetrail index refresh` over changed
    doc files — deterministic, git-native.
  - **(Deferred)** an optional **ft-ui-owned file watcher** for the live-editing loop —
    placed in the long-lived ft-ui server, *not* the idle-timing-out embed daemon, and
    built so the system is fully correct without it. Off/crashed/Windows → lazy-on-read
    silently covers.

The design gives the dead `content_hash` concept a real job and re-embeds **only on real
change**.

### 3.6 UI: docs-on-ticket is the headline

Of the candidate ft-ui surfaces, the **docs-on-ticket panel** ships first because it is
*delivery* (get the right doc to whoever works the ticket) rather than *management*
(curate a corpus), and it is the direct expression of the goal. Almost every surface
reuses what ft-ui already ships (board/drawer, memory browser, force-graph, trust-color
language, tiptap markdown editor, ⌘K palette).

Phase 3 (this spec): the ticket drawer gains a **Docs** panel listing `DocumentedIn`
docs, rendered inline, each with a **stale badge** driven by `content_hash` mismatch.
Editing a doc in the UI writes through ops → re-indexes synchronously (the watcher-free
path).

Follow-on (not this spec): docs **library + freshness** view (mirror of memory browser,
filter by `doc_type`/scope/trust), **graph coverage** (docs as nodes, `DocumentedIn`
edges → "epic with no design doc," orphan docs), **prime-preview** ("show what a fresh
agent gets for this task").

## 4. Component impact

| Crate / area | Change |
|---|---|
| `ft-core` | `RecordKind::Doc` + `RecordBody::Doc` (path, content_hash, title, summary, doc_type, trust, scope); add `DocumentedIn`/`ImplementedBy` to the writable relation subset |
| `ft-storage` | `.firetrail/records/doc/` partition |
| `ft-index` | walk doc relation edges; `content_hash` drift detection used by lazy refresh |
| `ft-embed` / `ft-search` | `record_text` for `Doc` reads the linked file; FTS + vector over file text |
| `ft-prime` | walk `DocumentedIn` from target; include linked docs as required items (link + summary + path); lazy `content_hash` freshness before serving; broken-link handling |
| `ft-cli` | `firetrail doc add <file> --type <t>` / `doc link <doc> <work-item>` / `doc index [path]`; git-hook warmer wiring |
| `ft-ui` | ticket-drawer **Docs** panel; inline render; stale badge; edit-through-ops |
| git hooks | post-commit warmer: `firetrail index refresh` over changed doc files |

## 5. Convention layer (Phase 1 — usable today, code-free)

A frontmatter block at the top of each doc that the eventual `firetrail doc index`
reads verbatim:

```yaml
---
doc_type: design        # design | adr | runbook | reference (open)
status: draft           # maps to TrustState
links:                  # work items this doc documents
  - firetrail-n3gh
scope: ft-ui            # optional owning scope
---
```

`docs/` keeps its current shape (`ARCHITECTURE.md`, `ROADMAP.md`, `decisions/`,
`components/`, `plans/`, `superpowers/specs/`); the frontmatter adds the machine-readable
link without reorganizing anything.

## 6. Non-goals (YAGNI)

- No enforced taxonomy / no Spec-vs-Plan split / no first-class Roadmap tier.
- No daemon-based filesystem watcher in v1 (deferred, optional, ft-ui-owned).
- No migration of existing `Decision`/`Runbook` native kinds to the file-backed model
  (can fold in later if the model proves out).
- No external-system adapters (Jira/Confluence) — consistent with ADR-0014 addendum;
  agents pipe markdown in.
- No second copy of prose in the record (single source of truth is the file).

## 7. Open questions for the plan phase

- `doc_type` storage: a typed `String` field on the `Doc` body vs. an envelope `Label`.
- Hash function for `content_hash`: reuse BLAKE3 (matches the embed cache) vs. SHA-256
  (matches `state_hash`).
- Path semantics: enforce repo-relative; behavior when a doc lives outside the repo.
- Lazy-refresh write amplification: re-indexing during a read mutates `index.db` —
  confirm this is acceptable on the `search`/`prime` path or queue it.
- Whether `firetrail doc add` should *create* the `.md` from a template, or only adopt an
  existing file.

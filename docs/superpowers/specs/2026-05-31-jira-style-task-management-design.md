# Jira-style task management for the Firetrail GUI

**Date:** 2026-05-31
**Status:** Approved (brainstorm)
**Scope:** `ft-ui` web app (board, backlog, epics, detail drawer), with
supporting changes in `ft-ops` (board op) and `ft-index` (criteria counts).

## Problem

The Firetrail GUI has a single task-management surface — the kanban Board —
and it has two concrete defects plus a structural gap:

1. **Memory and docs leak into the board.** `ft_ops::tickets::board` builds an
   index query with `kinds: None` ("any kind"), so the 7 non-ticket record
   kinds (`Incident`, `Finding`, `Runbook`, `Decision`, `Gotcha`, `Memory`,
   `Doc`) are returned alongside tickets. Any with `Open`/`Ready` status land
   in the **Todo** column.
2. **The type pill never renders.** `board.tsx`'s `workType()` derives the
   Epic/Task/Bug pill by splitting `short_id` on `:`, but ids are formatted
   `TASK-<hex>` (uppercase prefix, `-` separator). The split never matches, so
   `workType()` always returns `null` and every card looks identical.
3. **No epic-level structure.** Epics exist as a record kind and via
   `parent_id`, but the UI exposes no epic grouping, no roll-up, no backlog,
   and no "is this epic done?" signal. Closing the last child of an epic does
   **not** close (or flag) the epic — `close()` operates on a single record
   with no parent cascade.

## Goals

- Tickets-only board, with working type pills.
- Epic-aware board: flat by default, "group by epic" swimlanes on demand,
  epic color stripes + filter chips.
- Information-rich cards (criteria progress, subtasks, blocked-by).
- A **Backlog** table view for triage/planning.
- An **Epics** roll-up view, including a "ready to close" nudge.
- A detail drawer that shows epic lineage and typed children/relations.

## Non-goals (YAGNI)

Sprints / boards-as-entities, story points, drag-to-reorder within a column
(no rank field exists), WIP limits, drag-to-reassign-epic, and **automatic**
epic closing. These are explicitly out of scope.

---

## Decisions (from brainstorm)

| # | Decision | Choice |
|---|----------|--------|
| Epic representation | A swimlanes / B flat+stripe / C hybrid | **C — hybrid** (flat default + group-by-epic toggle) |
| Card density | A compact / B standard / C rich | **C — rich** (progress bar, subtasks, blocked-by on card) |
| Views | A board-only / B +backlog / C +backlog+epics | **C — Board + Backlog + Epics** |
| Epic auto-close | A auto / B nudge / C manual | **B — nudge, no automatic writes** |
| Drag semantics | status-only vs status+epic | **status-only in v1** |
| Index enrichment | add criteria counts vs defer bar | **add counts to index** |

---

## Architecture

### 1. Index: criteria counts (`ft-index`)

`IndexedRecord` already carries `parent_id`, `blocked_by_count`,
`blocks_count`. Add two fields, computed at index time from the record's
acceptance criteria (criteria live on the record body — `Task`/`Subtask`/`Bug`
— per `ft-core/src/acceptance.rs`):

```rust
pub criteria_total: u32,   // count of acceptance criteria
pub criteria_met: u32,     // count with AcStatus::Checked
```

- Bump the index schema version; upgrade triggers a rebuild (the index is a
  derived cache, so this is safe — no data migration, just a re-scan).
- Records with no criteria report `0/0`; the UI renders no bar in that case.

This keeps the board a pure index read — **no per-card record loads**.

### 2. Board op: tickets-only + enriched cards (`ft-ops`)

**Bug fix:** on both the `ReadyQuery` and `ListQuery` paths in `board.rs`, set:

```rust
kinds: Some(vec![RecordKind::Epic, RecordKind::Task,
                 RecordKind::Subtask, RecordKind::Bug]),
```

**Enriched `BoardCard`** (extends the ts-rs–exported struct):

```rust
pub struct BoardCard {
    pub id: String,
    pub short_id: String,
    pub title: String,
    pub kind: String,            // NEW — drives the type pill (replaces string-parse)
    pub priority: String,
    pub owner: Option<String>,
    pub epic_id: Option<String>, // NEW — resolved by walking parent_id → Epic
    pub criteria_total: u32,     // NEW
    pub criteria_met: u32,       // NEW
    pub subtask_count: u32,      // NEW — children of kind Subtask
    pub blocked_by_count: u32,   // NEW — from the index
}
```

**Epic resolution.** For a card, walk `parent_id` upward until a record of
kind `Epic` is found (`Subtask → Task → Epic`; `Task/Bug → Epic`). Build a
`parent_id → record` map once from the already-fetched index rows so
resolution is in-memory and O(depth). `epic_id` is `None` for orphan tickets
and for epics themselves.

**`BoardOutput`** gains an epics descriptor so the frontend renders
stripes/chips/lanes without extra lookups:

```rust
pub struct BoardEpic { pub id: String, pub short_id: String, pub title: String }
pub struct BoardOutput {
    pub todo: Vec<BoardCard>,
    pub in_progress: Vec<BoardCard>,
    pub review: Vec<BoardCard>,
    pub done: Vec<BoardCard>,
    pub epics: Vec<BoardEpic>,   // NEW — every epic referenced by a card, + those with no children
}
```

Epic **color** is *not* stored — it is a deterministic hash of the epic id,
computed in the frontend (`epic-color.ts`), so colors are stable across
sessions with zero persistence.

### 3. Epics op (`ft-ops`)

New read-only `epics()` op + `GET /api/epics`, returning each epic with a
child roll-up:

```rust
pub struct EpicSummary {
    pub id: String,
    pub short_id: String,
    pub title: String,
    pub status: String,
    pub priority: String,
    pub child_total: u32,
    pub child_closed: u32,
    pub criteria_total: u32,     // epic's OWN acceptance criteria
    pub criteria_met: u32,
    pub ready_to_close: bool,    // child_total > 0 && child_closed == child_total
                                 //   && criteria_met == criteria_total && status != Closed
}
pub struct EpicsOutput { pub epics: Vec<EpicSummary>, pub children: BTreeMap<String, Vec<BoardCard>> }
```

`ready_to_close` is the data behind the **nudge** (decision B). It is purely a
derived flag; the op performs **no writes**.

### 4. Board view (`ft-ui` — the hybrid)

Default flat 4-column status grid. **Rich card** (`board-card.tsx`):

- Epic color stripe (left edge) + epic title chip (when `epic_id` set).
- Type pill driven by `card.kind` (fixes bug #2).
- Priority badge (existing `PriorityBadge`).
- Criteria progress bar + `✓ met/total` when `criteria_total > 0`.
- Subtask count `⛬ n` when `subtask_count > 0`.
- `⊘ blocked by` badge when `blocked_by_count > 0`.
- Owner avatar.

**Group-by-epic toggle** in the header re-lays the same cards into collapsible
swimlanes (`board-swimlanes.tsx`) — one lane per epic + a "No epic" lane, each
with a roll-up progress bar. Toggle state persisted to `localStorage`
(`ft-ui:board-group-by-epic`), mirroring the sidebar-collapse pattern.

**Epic filter chips** (`epic-chips.tsx`) row narrows to one/several epics.
Coexists with the existing `ready` (unblocked-only) toggle.

Drag-to-move changes **status only** in v1 (including within swimlane mode —
dragging across a lane does not reassign the epic).

### 5. Backlog view (`ft-ui` — new nav `≣ Backlog`, route `/backlog`)

Dense, sortable, filterable table of all tickets: type, id, title, epic,
priority, status, owner, criteria progress. Sortable columns; filters for
epic / owner / status / kind. Inline priority + status edit reusing existing
`useUpdateTicket`. Backed by `GET /api/tickets` (list), with the same field
enrichment echoed through as the board.

### 6. Epics view (`ft-ui` — new nav `◇ Epics`, route `/epics`)

One row per epic with a roll-up progress bar (children closed + criteria met)
and a child-status breakdown, expandable to its child tasks/bugs (typed,
titled, click-through). Each epic links through to the board filtered to that
epic.

**Ready-to-close nudge (decision B):** when `ready_to_close` is true, the epic
row shows an "All children done — close epic?" affordance with a **Close
epic** button that calls the existing `POST /api/tickets/:id/close`. No
automatic close; the user clicks.

### 7. Detail drawer upgrades (`ticket-detail.tsx`)

- **Epic breadcrumb** at the top (`◇ <epic title> › this ticket`) when
  `parent_id` resolves to an epic.
- **Children section** — typed, titled list of subtasks/child tasks with
  status + click-through (replaces raw-id relation rows for parent/child).
- Resolve remaining **relation rows to title + type pill** instead of
  `id.slice(0, 16)`.
- For an **epic** being viewed: show the same child roll-up + the ready-to-close
  nudge as the Epics view.

### 8. Component boundaries

`board.tsx` is already ~340 lines; splitting prevents a monolith:

- `features/tickets/board.tsx` — orchestration + dnd context (existing, slimmed)
- `features/tickets/board-card.tsx` — the rich card
- `features/tickets/board-swimlanes.tsx` — grouped layout
- `features/tickets/epic-chips.tsx` — epic filter chips
- `features/tickets/backlog.tsx` — backlog table
- `features/epics/` — epics view + api + query hook
- `features/tickets/epic-color.ts` — deterministic id→color hash (shared)

Sidebar (`sidebar.tsx`) `NAV` gains `Backlog` and `Epics` items between
`Board` and `Memory`.

## Data flow

```
ft-index (criteria_total/met, parent_id, blocked_by_count)
   └─> ft-ops::tickets::board  ─ kinds=[Epic,Task,Subtask,Bug], epic resolution ─> BoardOutput {columns, epics}
   └─> ft-ops::tickets::epics  ─ child roll-up, ready_to_close ─> EpicsOutput
        │
   ft-ui routes (/api/tickets/board, /api/epics, /api/tickets)
        │
   Board (flat | swimlanes)   Backlog (table)   Epics (roll-up + nudge)   Detail drawer (lineage)
```

## Error handling

- Board/epic resolution is in-memory over already-fetched rows; a missing
  `parent_id` target (dangling edge) resolves to `epic_id = None` rather than
  erroring.
- The close-epic nudge uses the existing close endpoint, inheriting its
  criteria validation and conflict handling (e.g. already-closed → 409).
- Index schema bump: if the rebuild fails, the board falls back to the
  existing error surface (`Board` already renders a load error state).

## Testing

**Rust**
- `board` excludes all 7 non-ticket kinds; only Epic/Task/Subtask/Bug appear.
- Epic resolution: subtask→task→epic yields the epic; orphan → `None`;
  dangling parent → `None`.
- `criteria_total`/`criteria_met` correct (none, partial, all-checked).
- `epics` op: `ready_to_close` true only when all children closed AND epic's
  own criteria met AND not already closed; false otherwise.
- Extend `crates/ft-ui/tests/tickets_routes.rs`; add `epics_routes.rs`.

**TypeScript (Vitest)**
- Card renders correct pill from `kind`, stripe/chip from `epic_id`, progress
  bar from counts, blocked badge from `blocked_by_count`.
- Swimlane grouping incl. "No epic" lane; group-by toggle persistence.
- Backlog sort + filter; inline edit calls mutation.
- `epic-color` determinism (same id → same color across runs).
- Epics view shows the nudge only when `ready_to_close`.

## Rollout

1. Index field add + schema bump (rebuild on upgrade).
2. Board op fix + enrichment (ships bug #1 fix immediately).
3. Board frontend: rich card (ships bug #2 fix) + swimlane toggle + chips.
4. Backlog view.
5. Epics view + nudge.
6. Detail drawer lineage.

Steps 1–3 are the core; 4–6 are additive and independently shippable.

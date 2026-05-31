# Jira-style Task Management Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the single kanban board into an epic-aware task-management suite — tickets-only board with hybrid epic grouping and rich cards, a Backlog table, an Epics roll-up view with a "ready to close" nudge, and a detail drawer that shows epic lineage — while fixing two latent board bugs.

**Architecture:** Enrich the index's `IndexedRecord` with criteria counts (query-only, no migration), enrich `BoardCard` in `ft-ops` (kind, epic_id resolved by walking `parent_id`, criteria/subtask/blocked counts) and add an `epics` op + `/api/epics` route. The React app gains a rich card, a group-by-epic swimlane layout, epic filter chips, a backlog table, and an epics view. TS wire types are regenerated from Rust via `cargo xtask gen-ts` — never hand-edited.

**Tech Stack:** Rust (axum, rusqlite, ts-rs), React 19 + TanStack Router/Query, dnd-kit, framer-motion, Tailwind, Vitest.

---

## Conventions for every task

- **ts-rs types are generated.** After changing any `#[derive(TS)]` struct, run `just ui-gen-ts` (`cargo xtask gen-ts`) and commit the regenerated files under `crates/ft-ui/web/src/api/types/`. Never edit those files by hand. New exported types must be registered in `xtask/src/main.rs`.
- **Rust tests:** `cargo test -p <crate>`. **Lints:** `cargo clippy --all-targets -- -D warnings && cargo fmt --all`.
- **Web tests:** from `crates/ft-ui/web`: `pnpm test` (Vitest). **Lint:** `pnpm lint`.
- **Drift gate:** `cargo xtask check-ts` must pass (CI runs it). Run it before each backend commit that touches a wire type.

---

## File Structure

**Rust — modify:**
- `crates/ft-index/src/types.rs` — add `criteria_total`/`criteria_met` to `IndexedRecord`.
- `crates/ft-index/src/index.rs` — add count subqueries to both SELECTs + read new columns in `row_to_record`.
- `crates/ft-ops/src/tickets/board.rs` — kinds filter, enriched `BoardCard`, epic resolution, `BoardEpic`, `epics` in `BoardOutput`.
- `crates/ft-ops/src/tickets/mod.rs` — export new epics op + types.
- `crates/ft-ui/src/routes/mod.rs` — mount `/api/epics`.
- `xtask/src/main.rs` — register `EpicSummary`, `EpicsOutput`, `BoardEpic`.

**Rust — create:**
- `crates/ft-ops/src/tickets/epics.rs` — `epics` op (`EpicSummary`, `EpicsOutput`).
- `crates/ft-ui/src/routes/epics.rs` — `GET /api/epics` handler.
- `crates/ft-ui/tests/epics_routes.rs` — route test.

**Web — create:**
- `crates/ft-ui/web/src/features/tickets/epic-color.ts` — deterministic id→color.
- `crates/ft-ui/web/src/features/tickets/board-card.tsx` — rich card.
- `crates/ft-ui/web/src/features/tickets/board-swimlanes.tsx` — grouped layout.
- `crates/ft-ui/web/src/features/tickets/epic-chips.tsx` — epic filter chips.
- `crates/ft-ui/web/src/features/tickets/backlog.tsx` — backlog table.
- `crates/ft-ui/web/src/features/epics/api.ts`, `use-epics-query.ts`, `epics-view.tsx`.
- `crates/ft-ui/web/src/routes/backlog.tsx`, `crates/ft-ui/web/src/routes/epics/index.tsx`.

**Web — modify:**
- `crates/ft-ui/web/src/features/tickets/board.tsx` — slim to orchestration; use new pieces.
- `crates/ft-ui/web/src/features/tickets/use-board-query.ts` — no change unless filters expand.
- `crates/ft-ui/web/src/features/tickets/ticket-detail.tsx` — epic breadcrumb + typed children.
- `crates/ft-ui/web/src/components/sidebar.tsx` — add Backlog + Epics nav items.

---

## PHASE 1 — Index criteria counts (`ft-index`)

### Task 1: Add criteria counts to `IndexedRecord`

**Files:**
- Modify: `crates/ft-index/src/types.rs` (`IndexedRecord`)
- Modify: `crates/ft-index/src/index.rs` (`build_list_sql`, ready SELECT, `row_to_record`)
- Test: `crates/ft-index/src/index.rs` (existing `#[cfg(test)]` module) or `crates/ft-index/tests/`

- [ ] **Step 1: Write the failing test.** Add to the ft-index test module a test that indexes a task with 3 criteria, 1 checked, then lists it and asserts the counts. Adapt the existing test helpers (find an existing `list` test for the fixture pattern).

```rust
#[test]
fn list_reports_criteria_counts() {
    let (idx, _tmp) = test_index_with_criteria(); // helper: indexes TASK with 3 ACs, 1 checked
    let rows = idx.list(&ListQuery { include_closed: true, ..Default::default() }).unwrap();
    let task = rows.iter().find(|r| r.kind == RecordKind::Task).unwrap();
    assert_eq!(task.criteria_total, 3);
    assert_eq!(task.criteria_met, 1);
}
```

- [ ] **Step 2: Run it — expect compile failure** (`no field criteria_total`).
Run: `cargo test -p ft-index list_reports_criteria_counts`
Expected: FAIL to compile.

- [ ] **Step 3: Add the fields** to `IndexedRecord` after `parent_id`:

```rust
    /// Total acceptance criteria attached to this record.
    pub criteria_total: u32,
    /// Acceptance criteria with status `checked`.
    pub criteria_met: u32,
```

- [ ] **Step 4: Add subqueries** to the non-count SELECT in `build_list_sql` (after the `bk` subquery, before `FROM records r`), and the identical pair to the `ready` query's SELECT:

```sql
                (SELECT COUNT(*) FROM acceptance_criteria WHERE record_id = r.id) AS ct,
                (SELECT COUNT(*) FROM acceptance_criteria WHERE record_id = r.id AND status = 'checked') AS cm,
```

(Insert before the existing `bb`/`bk` lines or after — keep column order consistent with the reader in Step 5.)

- [ ] **Step 5: Read the new columns** in `row_to_record`. The `bb`/`bk` are at indices 16/17; append the two new columns at 18/19 in BOTH SELECTs and read them:

```rust
    let ct: i64 = row.get(18)?;
    let cm: i64 = row.get(19)?;
```

and in the returned `IndexedRecord { … }`:

```rust
        criteria_total: u32::try_from(ct).unwrap_or(0),
        criteria_met: u32::try_from(cm).unwrap_or(0),
```

Verify the ready-query reader path uses the same column indices (if `ready` reuses `row_to_record`, the SELECT column order must match exactly; if it has its own mapper, update both).

- [ ] **Step 6: Run the test — expect PASS.**
Run: `cargo test -p ft-index list_reports_criteria_counts`
Expected: PASS.

- [ ] **Step 7: Run the whole crate + clippy.**
Run: `cargo test -p ft-index && cargo clippy -p ft-index --all-targets -- -D warnings`
Expected: PASS, no warnings.

- [ ] **Step 8: Commit.**

```bash
git add crates/ft-index/
git commit -m "feat(ft-index): expose acceptance-criteria counts on IndexedRecord"
```

---

## PHASE 2 — Board op: tickets-only + enriched cards (`ft-ops`)

### Task 2: Restrict the board to ticket kinds (bug #1 fix)

**Files:**
- Modify: `crates/ft-ops/src/tickets/board.rs` (`board` fn, both query paths)
- Test: `crates/ft-ops/src/tickets/board.rs` (`#[cfg(test)]`)

- [ ] **Step 1: Write the failing test.** Index a `Memory` and a `Decision` record (both `Open`) plus a `Task`, call `board`, assert only the task appears.

```rust
#[test]
fn board_excludes_memory_and_doc_kinds() {
    let (ws, ident, events) = board_fixture_with_memory(); // task + open memory + open decision
    let out = board(&ws, &ident, BoardInput::default(), &events).unwrap();
    let ids: Vec<_> = out.todo.iter().map(|c| c.kind.as_str()).collect();
    assert!(ids.iter().all(|k| matches!(*k, "epic"|"task"|"subtask"|"bug")));
    assert_eq!(out.todo.iter().filter(|c| c.kind == "task").count(), 1);
}
```

- [ ] **Step 2: Run — expect FAIL** (memory/decision leak into `todo`, and `kind` field missing → compile fail; that's fine, both fixed here + Task 3). If you prefer isolation, stub `kind` in Task 2 and enrich in Task 3 — but doing both together is cleaner. This plan does the kinds filter here and the struct enrichment in Task 3, so split the assertion: for Task 2 assert counts only.

Adjusted Task 2 assertion (no `kind` field yet):

```rust
    // Only the single task should appear across all columns.
    let total = out.todo.len() + out.in_progress.len() + out.review.len() + out.done.len();
    assert_eq!(total, 1);
```

Run: `cargo test -p ft-ops board_excludes_memory_and_doc_kinds`
Expected: FAIL (total == 3).

- [ ] **Step 3: Add the kinds filter.** In `board.rs`, import `ft_core::RecordKind` and set `kinds` on BOTH the `ReadyQuery` and `ListQuery`:

```rust
use ft_core::RecordKind;

const TICKET_KINDS: [RecordKind; 4] =
    [RecordKind::Epic, RecordKind::Task, RecordKind::Subtask, RecordKind::Bug];
```

In the `ready` branch: `rq.kinds = Some(TICKET_KINDS.to_vec());`
In the list branch: add `kinds: Some(TICKET_KINDS.to_vec()),` to the `ListQuery { … }` initializer.

- [ ] **Step 4: Run — expect PASS.**
Run: `cargo test -p ft-ops board_excludes_memory_and_doc_kinds`
Expected: PASS.

- [ ] **Step 5: Commit.**

```bash
git add crates/ft-ops/src/tickets/board.rs
git commit -m "fix(ft-ops): board returns only ticket kinds, not memory/docs (bug #1)"
```

### Task 3: Enrich `BoardCard` + add `BoardEpic` and epic resolution

**Files:**
- Modify: `crates/ft-ops/src/tickets/board.rs`
- Test: `crates/ft-ops/src/tickets/board.rs`

- [ ] **Step 1: Write the failing test** for epic resolution + counts.

```rust
#[test]
fn board_resolves_epic_and_counts() {
    // epic E; task T (parent E, 2 criteria 1 met); subtask S (parent T).
    let (ws, ident, events, e_id, t_id) = board_fixture_epic_chain();
    let out = board(&ws, &ident, BoardInput::default(), &events).unwrap();
    let t = find_card(&out, &t_id);
    assert_eq!(t.kind, "task");
    assert_eq!(t.epic_id.as_deref(), Some(e_id.as_str()));
    assert_eq!((t.criteria_total, t.criteria_met), (2, 1));
    assert_eq!(t.subtask_count, 1);
    // subtask resolves up through its task to the epic
    let s = out_all(&out).into_iter().find(|c| c.kind == "subtask").unwrap();
    assert_eq!(s.epic_id.as_deref(), Some(e_id.as_str()));
    // the epic card itself has no epic_id
    let e = find_card(&out, &e_id);
    assert_eq!(e.epic_id, None);
}
```

- [ ] **Step 2: Run — expect compile FAIL** (`kind`, `epic_id`, etc. missing).
Run: `cargo test -p ft-ops board_resolves_epic_and_counts`
Expected: FAIL to compile.

- [ ] **Step 3: Extend `BoardCard` and add `BoardEpic`.** Replace the struct and add the epics descriptor:

```rust
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BoardCard {
    pub id: String,
    pub short_id: String,
    pub title: String,
    /// Record kind, lowercase (`"epic"|"task"|"subtask"|"bug"`). Drives the type pill.
    pub kind: String,
    pub priority: String,
    pub owner: Option<String>,
    /// Canonical id of the enclosing epic, resolved by walking `parent_id`. `None` for orphans/epics.
    pub epic_id: Option<String>,
    pub criteria_total: u32,
    pub criteria_met: u32,
    /// Direct children of kind `Subtask`.
    pub subtask_count: u32,
    pub blocked_by_count: u32,
}

#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BoardEpic {
    pub id: String,
    pub short_id: String,
    pub title: String,
}
```

Add `pub epics: Vec<BoardEpic>,` to `BoardOutput`.

- [ ] **Step 4: Rewrite `build_board`** to resolve epics and counts from the fetched rows. It must take ownership of building the epic map:

```rust
fn build_board(rows: &[IndexedRecord]) -> BoardOutput {
    use std::collections::HashMap;
    // id -> (kind, parent_id) for upward walks.
    let by_id: HashMap<&str, &IndexedRecord> =
        rows.iter().map(|r| (r.id.as_str(), r)).collect();
    // direct subtask child counts.
    let mut subtasks: HashMap<&str, u32> = HashMap::new();
    for r in rows {
        if r.kind == RecordKind::Subtask {
            if let Some(p) = &r.parent_id {
                *subtasks.entry(p.as_str()).or_default() += 1;
            }
        }
    }

    fn resolve_epic<'a>(start: &'a IndexedRecord, by_id: &HashMap<&str, &'a IndexedRecord>) -> Option<String> {
        let mut cur = start;
        // Walk up to 8 hops to avoid pathological/cyclic chains.
        for _ in 0..8 {
            if cur.kind == RecordKind::Epic { return Some(cur.id.as_str().to_string()); }
            let parent = cur.parent_id.as_ref()?;
            cur = by_id.get(parent.as_str())?;
        }
        None
    }

    let (mut todo, mut in_progress, mut review, mut done) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut epics: Vec<BoardEpic> = Vec::new();

    for r in rows {
        if r.kind == RecordKind::Epic {
            epics.push(BoardEpic {
                id: r.id.as_str().to_string(),
                short_id: r.id.short(8).to_string(),
                title: r.title.clone(),
            });
        }
        // epic_id is the resolved epic UNLESS this record IS the epic.
        let epic_id = if r.kind == RecordKind::Epic { None } else { resolve_epic(r, &by_id) };
        let card = BoardCard {
            id: r.id.as_str().to_string(),
            short_id: r.id.short(8).to_string(),
            title: r.title.clone(),
            kind: format!("{:?}", r.kind).to_lowercase(),
            priority: format!("{:?}", r.priority).to_lowercase(),
            owner: r.owner.as_ref().map(|o| o.as_str().to_string()),
            epic_id,
            criteria_total: r.criteria_total,
            criteria_met: r.criteria_met,
            subtask_count: *subtasks.get(r.id.as_str()).unwrap_or(&0),
            blocked_by_count: r.blocked_by_count,
        };
        match r.status {
            Status::Open | Status::Ready => todo.push(card),
            Status::InProgress | Status::Blocked => in_progress.push(card),
            Status::Review => review.push(card),
            Status::Closed => done.push(card),
            _ => {}
        }
    }
    for col in [&mut todo, &mut in_progress, &mut review, &mut done] {
        col.sort_by(|a, b| a.id.cmp(&b.id));
    }
    epics.sort_by(|a, b| a.title.cmp(&b.title));
    BoardOutput { todo, in_progress, review, done, epics }
}
```

Note: `format!("{:?}", r.kind).to_lowercase()` yields `"epic"|"task"|"subtask"|"bug"` because `RecordKind`'s Debug names match (verify `Subtask` → `"subtask"`; it does). If you prefer not to rely on Debug, add a small `kind_str` match.

- [ ] **Step 5: Run the test — expect PASS.**
Run: `cargo test -p ft-ops board_resolves_epic_and_counts board_excludes_memory_and_doc_kinds`
Expected: PASS (update Task 2's test back to asserting `c.kind` now that the field exists).

- [ ] **Step 6: Regenerate TS bindings + verify no drift.**
Run: `just ui-gen-ts && cargo xtask check-ts`
Expected: `BoardCard.ts`, `BoardOutput.ts`, `BoardEpic.ts` updated; check-ts passes.

- [ ] **Step 7: clippy + commit.**

```bash
cargo clippy -p ft-ops --all-targets -- -D warnings
git add crates/ft-ops/src/tickets/board.rs crates/ft-ui/web/src/api/types/
git commit -m "feat(ft-ops): enrich BoardCard (kind, epic_id, criteria/subtask/blocked counts)"
```

---

## PHASE 3 — Board frontend: rich card + swimlanes + chips

### Task 4: Deterministic epic color util

**Files:**
- Create: `crates/ft-ui/web/src/features/tickets/epic-color.ts`
- Test: `crates/ft-ui/web/src/features/tickets/epic-color.test.ts`

- [ ] **Step 1: Write the failing test.**

```ts
import { describe, it, expect } from 'vitest'
import { epicColor } from './epic-color'

describe('epicColor', () => {
  it('is deterministic for the same id', () => {
    expect(epicColor('EPIC-abc')).toBe(epicColor('EPIC-abc'))
  })
  it('returns an hsl string', () => {
    expect(epicColor('EPIC-abc')).toMatch(/^hsl\(/)
  })
})
```

- [ ] **Step 2: Run — expect FAIL** (module missing).
Run (from `crates/ft-ui/web`): `pnpm test epic-color`
Expected: FAIL.

- [ ] **Step 3: Implement.**

```ts
/** Deterministic, stable color for an epic id. No persistence — pure hash → hue. */
export function epicColor(epicId: string): string {
  let h = 0
  for (let i = 0; i < epicId.length; i++) {
    h = (h * 31 + epicId.charCodeAt(i)) >>> 0
  }
  const hue = h % 360
  return `hsl(${hue} 65% 60%)`
}
```

- [ ] **Step 4: Run — expect PASS.** Run: `pnpm test epic-color` → PASS.

- [ ] **Step 5: Commit.**

```bash
git add crates/ft-ui/web/src/features/tickets/epic-color.*
git commit -m "feat(ft-ui): deterministic epic color util"
```

### Task 5: Rich board card component

**Files:**
- Create: `crates/ft-ui/web/src/features/tickets/board-card.tsx`
- Test: `crates/ft-ui/web/src/features/tickets/board-card.test.tsx`
- Modify: `crates/ft-ui/web/src/features/tickets/board.tsx` (extract `DraggableCard` to use the new card body; keep dnd wrapper)

- [ ] **Step 1: Write the failing test.**

```tsx
import { render, screen } from '@testing-library/react'
import { describe, it, expect } from 'vitest'
import { BoardCardBody } from './board-card'
import type { BoardCard } from '@/api/types/BoardCard'

const card: BoardCard = {
  id: 'TASK-1', short_id: 'TASK-1', title: 'Reset flow', kind: 'task',
  priority: 'p2', owner: 'alice', epic_id: 'EPIC-9',
  criteria_total: 5, criteria_met: 3, subtask_count: 2, blocked_by_count: 1,
}

describe('BoardCardBody', () => {
  it('renders the type pill from kind', () => {
    render(<BoardCardBody card={card} epicTitle="Ship v1" />)
    expect(screen.getByText(/task/i)).toBeInTheDocument()
  })
  it('renders criteria progress and blocked badge', () => {
    render(<BoardCardBody card={card} epicTitle="Ship v1" />)
    expect(screen.getByText('3/5')).toBeInTheDocument()
    expect(screen.getByText(/blocked/i)).toBeInTheDocument()
  })
})
```

(Wrap in a router stub if `<Link>` errors — reuse the test setup pattern from the existing `board.test.tsx`.)

- [ ] **Step 2: Run — expect FAIL.** Run: `pnpm test board-card` → FAIL.

- [ ] **Step 3: Implement `BoardCardBody`** (presentational; the draggable wrapper stays in `board.tsx`). Drive the pill from `card.kind` (fixes bug #2):

```tsx
import { Link } from '@tanstack/react-router'
import { Badge, type BadgeProps } from '@/components/ui/badge'
import { cn } from '@/lib/utils'
import { epicColor } from './epic-color'
import { PriorityBadge } from './board'
import type { BoardCard } from '@/api/types/BoardCard'

const KIND_VARIANT: Record<string, BadgeProps['variant']> = {
  epic: 'epic', task: 'task', subtask: 'task', bug: 'bug', feature: 'feature',
}

export function BoardCardBody({ card, epicTitle }: { card: BoardCard; epicTitle?: string }) {
  const variant = KIND_VARIANT[card.kind] ?? 'secondary'
  const pct = card.criteria_total > 0 ? Math.round((card.criteria_met / card.criteria_total) * 100) : 0
  return (
    <div className="flex flex-col gap-2.5">
      <div className="flex items-center justify-between gap-2">
        <div className="flex min-w-0 items-center gap-2">
          <Badge variant={variant} className="px-1.5 py-0 text-[0.625rem] capitalize">{card.kind}</Badge>
          {card.epic_id && epicTitle && (
            <span className="truncate rounded-full px-1.5 py-0.5 text-[0.625rem]"
                  style={{ background: `${epicColor(card.epic_id)}22`, color: epicColor(card.epic_id) }}>
              {epicTitle}
            </span>
          )}
        </div>
        <PriorityBadge priority={card.priority} />
      </div>
      <Link to="/tickets/$id" params={{ id: card.id }}
            className="block text-sm font-medium leading-snug text-foreground hover:text-primary"
            onPointerDown={(e) => e.stopPropagation()}>
        {card.title}
      </Link>
      {card.criteria_total > 0 && (
        <div className="flex items-center gap-2">
          <div className="h-1 flex-1 overflow-hidden rounded-full bg-muted">
            <div className="h-full rounded-full bg-type-task" style={{ width: `${pct}%` }} />
          </div>
          <span className="font-mono text-[0.625rem] text-muted-foreground">{card.criteria_met}/{card.criteria_total}</span>
        </div>
      )}
      <div className="flex items-center gap-2 text-[0.625rem] text-muted-foreground">
        <span className="font-mono">{card.short_id}</span>
        {card.subtask_count > 0 && <span>⛬ {card.subtask_count}</span>}
        {card.blocked_by_count > 0 && (
          <span className="rounded-full bg-destructive/15 px-1.5 py-0.5 text-destructive">⊘ blocked</span>
        )}
        {card.owner && <span className="ml-auto truncate">{card.owner}</span>}
      </div>
    </div>
  )
}
```

- [ ] **Step 4: Wire it into `board.tsx`.** In `DraggableCard`, replace the inner markup (the `flex flex-col` block from the type pill through owner) with `<BoardCardBody card={card} epicTitle={epicTitleFor(card.epic_id)} />`. Thread an `epics` lookup map from `data.epics` down to the column → card. Remove the now-dead `workType()` function and the broken `:`-split (bug #2 fully removed).

- [ ] **Step 5: Run card + board tests — expect PASS.**
Run: `pnpm test board-card board.test` → PASS. Fix the existing `board.test.tsx` expectations if they asserted the old markup.

- [ ] **Step 6: Lint + commit.**

```bash
pnpm lint
git add crates/ft-ui/web/src/features/tickets/board-card.tsx crates/ft-ui/web/src/features/tickets/board-card.test.tsx crates/ft-ui/web/src/features/tickets/board.tsx
git commit -m "feat(ft-ui): rich board card with type pill (bug #2), epic chip, criteria progress, blocked badge"
```

### Task 6: Epic filter chips

**Files:**
- Create: `crates/ft-ui/web/src/features/tickets/epic-chips.tsx`
- Test: `crates/ft-ui/web/src/features/tickets/epic-chips.test.tsx`
- Modify: `crates/ft-ui/web/src/features/tickets/board.tsx`

- [ ] **Step 1: Write the failing test.** Render chips for two epics + "No epic", click one, assert `onChange` fires with that epic id; click again deselects.

```tsx
import { render, screen, fireEvent } from '@testing-library/react'
import { describe, it, expect, vi } from 'vitest'
import { EpicChips } from './epic-chips'

it('toggles an epic on click', () => {
  const onChange = vi.fn()
  render(<EpicChips epics={[{ id: 'EPIC-1', short_id: 'EPIC-1', title: 'Auth' }]} selected={new Set()} onChange={onChange} />)
  fireEvent.click(screen.getByText('Auth'))
  expect(onChange).toHaveBeenCalledWith(expect.any(Set))
})
```

- [ ] **Step 2: Run — expect FAIL.** Run: `pnpm test epic-chips` → FAIL.

- [ ] **Step 3: Implement** a controlled chip row: props `{ epics: BoardEpic[]; selected: Set<string>; onChange: (next: Set<string>) => void }`. Each chip toggles its id in a cloned Set and calls `onChange`. Use `epicColor` for the active tint. Include a "No epic" chip whose sentinel id is the empty string `''`.

- [ ] **Step 4: Wire into `board.tsx`.** Hold `const [epicFilter, setEpicFilter] = useState<Set<string>>(new Set())`. When non-empty, filter each column's cards: keep a card if `selected.has(card.epic_id ?? '')`. Render `<EpicChips>` in the header below `PageHeader`.

- [ ] **Step 5: Run — expect PASS.** Run: `pnpm test epic-chips board.test` → PASS.

- [ ] **Step 6: Commit.**

```bash
git add crates/ft-ui/web/src/features/tickets/epic-chips.*  crates/ft-ui/web/src/features/tickets/board.tsx
git commit -m "feat(ft-ui): epic filter chips on the board"
```

### Task 7: Group-by-epic swimlanes + toggle

**Files:**
- Create: `crates/ft-ui/web/src/features/tickets/board-swimlanes.tsx`
- Test: `crates/ft-ui/web/src/features/tickets/board-swimlanes.test.tsx`
- Modify: `crates/ft-ui/web/src/features/tickets/board.tsx`

- [ ] **Step 1: Write the failing test.** Given a `BoardOutput` with cards across two epics + one orphan, assert the swimlane component renders a lane per epic plus a "No epic" lane, each containing the right cards.

```tsx
import { render, screen } from '@testing-library/react'
import { describe, it, expect } from 'vitest'
import { groupByEpic } from './board-swimlanes'

it('groups cards into epic lanes + a no-epic lane', () => {
  const out = {
    todo: [{ id:'T1', epic_id:'E1' }, { id:'T2', epic_id:null }],
    in_progress: [], review: [], done: [],
    epics: [{ id:'E1', short_id:'E1', title:'Auth' }],
  } as any
  const lanes = groupByEpic(out)
  expect(lanes.map(l => l.key)).toEqual(['E1', ''])
  expect(lanes[0].columns.todo).toHaveLength(1)
  expect(lanes[1].columns.todo).toHaveLength(1)
})
```

- [ ] **Step 2: Run — expect FAIL.** Run: `pnpm test board-swimlanes` → FAIL.

- [ ] **Step 3: Implement `groupByEpic(out): Lane[]`** (pure, exported for test) plus a `<BoardSwimlanes>` component that renders each lane as a collapsible row of the four droppable columns, reusing `DroppableColumn`/`DraggableCard` from `board.tsx` (export them). A `Lane` is `{ key: string; title: string; columns: BoardOutput }`. Lane order: epics in `out.epics` order, then `''` (No epic) last. Each lane header shows a roll-up `met/total` summed across its cards.

- [ ] **Step 4: Add the toggle to `board.tsx`.** `const [groupByEpicOn, setGroupByEpicOn] = useState(readGrouping())` persisted to `localStorage['ft-ui:board-group-by-epic']` (mirror the sidebar `readCollapsed`/effect pattern). Header gets a toggle button (like the existing `ready` toggle). When on, render `<BoardSwimlanes>` inside the same `DndContext`; when off, render the existing 4-column grid. Drag still maps `over.id` → status column and moves status only.

- [ ] **Step 5: Run — expect PASS.** Run: `pnpm test board-swimlanes board.test` → PASS.

- [ ] **Step 6: Lint + commit.**

```bash
pnpm lint
git add crates/ft-ui/web/src/features/tickets/board-swimlanes.* crates/ft-ui/web/src/features/tickets/board.tsx
git commit -m "feat(ft-ui): group-by-epic swimlanes with persisted toggle"
```

---

## PHASE 4 — Backlog view

### Task 8: Backlog nav item + route + table

**Files:**
- Modify: `crates/ft-ui/web/src/components/sidebar.tsx` (add nav item)
- Create: `crates/ft-ui/web/src/routes/backlog.tsx` (TanStack route)
- Create: `crates/ft-ui/web/src/features/tickets/backlog.tsx`
- Test: `crates/ft-ui/web/src/features/tickets/backlog.test.tsx`

- [ ] **Step 1: Write the failing test.** Render `<Backlog>` with a mocked board query returning 3 cards across columns; assert all 3 rows render and that clicking the "Priority" header sorts (assert order changes).

```tsx
import { render, screen, fireEvent, within } from '@testing-library/react'
import { describe, it, expect } from 'vitest'
import { sortRows, type BacklogRow } from './backlog'

it('sorts rows by priority ascending then descending', () => {
  const rows: BacklogRow[] = [
    { id:'A', kind:'task', title:'a', priority:'p3', status:'open', epic_id:null, owner:null, criteria_total:0, criteria_met:0 } as any,
    { id:'B', kind:'bug',  title:'b', priority:'p0', status:'open', epic_id:null, owner:null, criteria_total:0, criteria_met:0 } as any,
  ]
  expect(sortRows(rows, 'priority', 'asc').map(r => r.id)).toEqual(['B','A'])
  expect(sortRows(rows, 'priority', 'desc').map(r => r.id)).toEqual(['A','B'])
})
```

- [ ] **Step 2: Run — expect FAIL.** Run: `pnpm test backlog` → FAIL.

- [ ] **Step 3: Implement `sortRows` + `<Backlog>`.** Flatten the existing `useBoardQuery` columns into one `BacklogRow[]` (each card already carries everything the table shows: kind, title, priority, status via its column, epic_id, owner, criteria counts — derive `status` from which column the card came in). Provide a pure exported `sortRows(rows, key, dir)`. Render a `Table` (existing `components/ui/table.tsx`) with sortable headers and filter selects (epic/owner/status/kind). Inline priority + status edit reuse `useUpdateTicket`. Row click → `/tickets/$id`.

Note: reuse `useBoardQuery` (already returns all columns incl. closed). No new endpoint needed for v1; the board op now carries all fields the table needs.

- [ ] **Step 4: Add the route.** `crates/ft-ui/web/src/routes/backlog.tsx`:

```tsx
import { createFileRoute } from '@tanstack/react-router'
import { Backlog } from '@/features/tickets/backlog'
export const Route = createFileRoute('/backlog')({ component: Backlog })
```

Run the router codegen if the project uses it (TanStack auto-generates `routeTree.gen.ts` via the vite plugin on `pnpm dev`/`pnpm build`; otherwise run the documented gen command). Verify `routeTree.gen.ts` picks up `/backlog`.

- [ ] **Step 5: Add the nav item.** In `sidebar.tsx`, add to `NAV` after Board: `{ to: '/backlog', label: 'Backlog', icon: ListTodo }` (import `ListTodo` from lucide-react).

- [ ] **Step 6: Run — expect PASS.** Run: `pnpm test backlog && pnpm build` → PASS (build proves route + types compile).

- [ ] **Step 7: Lint + commit.**

```bash
pnpm lint
git add crates/ft-ui/web/src/routes/backlog.tsx crates/ft-ui/web/src/features/tickets/backlog.* crates/ft-ui/web/src/components/sidebar.tsx crates/ft-ui/web/src/routeTree.gen.ts
git commit -m "feat(ft-ui): backlog table view"
```

---

## PHASE 5 — Epics view + ready-to-close nudge

### Task 9: `epics` op (`ft-ops`)

**Files:**
- Create: `crates/ft-ops/src/tickets/epics.rs`
- Modify: `crates/ft-ops/src/tickets/mod.rs` (declare `mod epics;`, export types + fn)
- Test: `crates/ft-ops/src/tickets/epics.rs` (`#[cfg(test)]`)

- [ ] **Step 1: Write the failing test.**

```rust
#[test]
fn epics_flags_ready_to_close() {
    // Epic E (0 own criteria), 2 children both Closed → ready_to_close = true.
    let (ws, ident, events, e_id) = epics_fixture_all_children_closed();
    let out = epics(&ws, &ident, EpicsInput::default(), &events).unwrap();
    let e = out.epics.iter().find(|e| e.id == e_id.as_str()).unwrap();
    assert_eq!((e.child_total, e.child_closed), (2, 2));
    assert!(e.ready_to_close);
}

#[test]
fn epics_not_ready_when_child_open() {
    let (ws, ident, events, e_id) = epics_fixture_one_child_open();
    let out = epics(&ws, &ident, EpicsInput::default(), &events).unwrap();
    let e = out.epics.iter().find(|e| e.id == e_id.as_str()).unwrap();
    assert!(!e.ready_to_close);
}
```

- [ ] **Step 2: Run — expect FAIL** (module missing).
Run: `cargo test -p ft-ops epics_flags_ready_to_close`
Expected: FAIL.

- [ ] **Step 3: Implement the op.** Reuse `TicketCtx` and the index. List with `kinds = [Epic]` for the epics; list children via `ListQuery { parent: Some(epic_id), include_closed: true, .. }` or by scanning all ticket rows and grouping by resolved epic (reuse the walk from `board.rs` — consider extracting `resolve_epic` into `tickets/ctx.rs` or a shared `tickets::epic_of` helper to keep DRY). For each epic compute:

```rust
pub struct EpicSummary {
    pub id: String, pub short_id: String, pub title: String,
    pub status: String, pub priority: String,
    pub child_total: u32, pub child_closed: u32,
    pub criteria_total: u32, pub criteria_met: u32, // epic's OWN criteria (from IndexedRecord)
    pub ready_to_close: bool,
}
```

`ready_to_close = child_total > 0 && child_closed == child_total && criteria_met == criteria_total && status != "closed"`.

`EpicsOutput { epics: Vec<EpicSummary>, children: BTreeMap<String, Vec<BoardCard>> }` where `children` maps epic id → its child cards (reuse `BoardCard`). Derive child cards with the same builder logic as the board (extract a `card_from(&IndexedRecord, …)` helper shared with `board.rs`).

Add `#[derive(ts_rs::TS)]` + `ts(export)` to `EpicSummary` and `EpicsOutput`. Add `EpicsInput` (empty `Default` struct, optional `scope`).

- [ ] **Step 4: Export** from `mod.rs`: `mod epics;` and `pub use epics::{EpicSummary, EpicsOutput, EpicsInput, epics};`.

- [ ] **Step 5: Run — expect PASS.** Run: `cargo test -p ft-ops epics_` → PASS.

- [ ] **Step 6: clippy + commit.**

```bash
cargo clippy -p ft-ops --all-targets -- -D warnings
git add crates/ft-ops/src/tickets/
git commit -m "feat(ft-ops): epics roll-up op with ready_to_close flag"
```

### Task 10: `/api/epics` route + ts-rs registration

**Files:**
- Create: `crates/ft-ui/src/routes/epics.rs`
- Modify: `crates/ft-ui/src/routes/mod.rs` (`pub mod epics;`, `.nest("/epics", epics::router())`)
- Modify: `xtask/src/main.rs` (register `EpicSummary`, `EpicsOutput`, `BoardEpic` for export)
- Test: `crates/ft-ui/tests/epics_routes.rs`

- [ ] **Step 1: Write the failing test.** Mirror `tickets_routes.rs`: spin up the test server, create an epic + closed child, `GET /api/epics`, assert 200 and that the epic appears with `ready_to_close` true.

- [ ] **Step 2: Run — expect FAIL.** Run: `cargo test -p ft-ui --test epics_routes` → FAIL.

- [ ] **Step 3: Implement the handler** (copy the shape of `board_handler` in `tickets.rs`): resolve identity, call `ft_ops::tickets::epics`, return `Json`. `router()` exposes `GET /`.

- [ ] **Step 4: Mount it** in `routes/mod.rs`: `pub mod epics;` and `.nest("/epics", epics::router())` in the `api` router.

- [ ] **Step 5: Register ts-rs types.** In `xtask/src/main.rs`, add `EpicSummary`, `EpicsOutput`, `BoardEpic` to the export list alongside the existing `BoardCard`/`BoardOutput` entries (follow the existing `export!`/`.export()` pattern in that file).

- [ ] **Step 6: Regenerate + verify.** Run: `just ui-gen-ts && cargo xtask check-ts` → new `EpicSummary.ts`, `EpicsOutput.ts` generated, drift gate passes.

- [ ] **Step 7: Run — expect PASS.** Run: `cargo test -p ft-ui --test epics_routes` → PASS.

- [ ] **Step 8: Commit.**

```bash
git add crates/ft-ui/src/routes/epics.rs crates/ft-ui/src/routes/mod.rs xtask/src/main.rs crates/ft-ui/web/src/api/types/
git commit -m "feat(ft-ui): GET /api/epics route + ts bindings"
```

### Task 11: Epics view + nudge (frontend)

**Files:**
- Create: `crates/ft-ui/web/src/features/epics/api.ts`, `use-epics-query.ts`, `epics-view.tsx`
- Create: `crates/ft-ui/web/src/routes/epics/index.tsx`
- Modify: `crates/ft-ui/web/src/components/sidebar.tsx`
- Test: `crates/ft-ui/web/src/features/epics/epics-view.test.tsx`

- [ ] **Step 1: Write the failing test.** Render `<EpicsView>` with mocked query returning one epic `ready_to_close: true` and one `false`; assert the "Close epic" button shows only for the ready one.

```tsx
it('shows the close-epic nudge only when ready', () => {
  // mock useEpicsQuery → { epics: [{id:'E1', ready_to_close:true,...},{id:'E2', ready_to_close:false,...}], children:{} }
  render(<EpicsView />)
  expect(screen.getAllByRole('button', { name: /close epic/i })).toHaveLength(1)
})
```

- [ ] **Step 2: Run — expect FAIL.** Run: `pnpm test epics-view` → FAIL.

- [ ] **Step 3: Implement** `fetchEpics()` (GET `/api/epics`), `useEpicsQuery` (TanStack, mirror `use-board-query.ts`), and `<EpicsView>`: one row per epic with a roll-up progress bar (`child_closed/child_total` + criteria), expandable to `children[epic.id]` cards (reuse `BoardCardBody`), each epic linking to `/?epic=<id>` (or board with chip preselected). When `epic.ready_to_close`, render a **Close epic** button calling `closeTicket(epic.id)` (existing api) with a confirm, then invalidate the epics + board queries.

- [ ] **Step 4: Add route + nav.** `routes/epics/index.tsx` (createFileRoute `/epics`); add `{ to: '/epics', label: 'Epics', icon: Diamond }` to sidebar `NAV` after Backlog.

- [ ] **Step 5: Run — expect PASS.** Run: `pnpm test epics-view && pnpm build` → PASS.

- [ ] **Step 6: Lint + commit.**

```bash
pnpm lint
git add crates/ft-ui/web/src/features/epics/ crates/ft-ui/web/src/routes/epics/ crates/ft-ui/web/src/components/sidebar.tsx crates/ft-ui/web/src/routeTree.gen.ts
git commit -m "feat(ft-ui): epics roll-up view with ready-to-close nudge"
```

---

## PHASE 6 — Detail drawer lineage

### Task 12: Epic breadcrumb + typed children in the detail drawer

**Files:**
- Modify: `crates/ft-ui/web/src/features/tickets/ticket-detail.tsx`
- Test: `crates/ft-ui/web/src/features/tickets/ticket-detail.test.tsx` (extend existing)

- [ ] **Step 1: Write the failing test.** Render `TicketDetail` for a task whose record resolves a parent epic; assert a breadcrumb link to the epic renders, and that a child subtask appears by title (not raw id).

```tsx
it('shows the epic breadcrumb and typed children', async () => {
  // mock useTicketQuery → record with parent epic + a child subtask relation
  render(<TicketDetail id="TASK-1" />)
  expect(await screen.findByText('Ship v1 auth')).toBeInTheDocument()   // breadcrumb
  expect(screen.getByText('Migrate schema')).toBeInTheDocument()        // child title, not id
})
```

- [ ] **Step 2: Run — expect FAIL.** Run: `pnpm test ticket-detail` → FAIL.

- [ ] **Step 3: Implement.** In `DetailBody`:
  - If `env.parent_id` (or the parent relation) resolves to an epic, render a breadcrumb `◇ <epic title> › <this title>` at the top, linking to `/tickets/$id`. Fetch the parent's title via a light `fetchTicket(parentId)` query (or include parent title in the show output if already present — check `ShowOutputWire`).
  - Replace parent/child rows in `RelationsPanel` with a **Children** section that resolves each child id to its title + type pill (a small `useTicketQuery` per child, or a batched lookup). Keep non-hierarchical relations in the existing list but resolve their display to title + pill instead of `id.slice(0,16)`.
  - For an epic record, show its child roll-up + the same ready-to-close nudge (reuse the epics query filtered to this id, or a derived check).

- [ ] **Step 4: Run — expect PASS.** Run: `pnpm test ticket-detail` → PASS.

- [ ] **Step 5: Lint + commit.**

```bash
pnpm lint
git add crates/ft-ui/web/src/features/tickets/ticket-detail.*
git commit -m "feat(ft-ui): detail drawer epic breadcrumb + typed children/relations"
```

---

## Final verification

- [ ] `cargo test --workspace` — all Rust tests pass.
- [ ] `cargo clippy --all-targets -- -D warnings && cargo fmt --all --check` — clean.
- [ ] `cargo xtask check-ts` — no TS binding drift.
- [ ] From `crates/ft-ui/web`: `pnpm test && pnpm lint && pnpm build` — green.
- [ ] Manual smoke (`/run` or `cargo run -p ft-ui`): board shows only tickets (no memory/docs in Todo), type pills render, group-by-epic toggle works, backlog sorts/filters, epics view shows nudge on a fully-closed epic, detail drawer shows breadcrumb.

## Notes on DRY

`resolve_epic` (parent walk) and `card_from(&IndexedRecord)` are used by both `board.rs` and `epics.rs`. Extract them into a shared spot in `crates/ft-ops/src/tickets/` (e.g. `ctx.rs` or a new `cards.rs`) the first time the second consumer needs them (Task 9), rather than copy-pasting.

/**
 * Group-by-epic swimlanes for the kanban board.
 *
 * `groupByEpic` is a pure function (exported for testing) that reshapes a
 * `BoardOutput` into an ordered list of `Lane` objects — one per epic that has
 * ≥1 card, plus an optional "No epic" sentinel lane for orphan cards.
 *
 * `<BoardSwimlanes>` renders those lanes as collapsible horizontal rows, each
 * containing the four droppable status columns. Droppable ids are composite:
 * `"${laneKey}::${column}"` to satisfy dnd-kit's uniqueness requirement.
 */
import { useState } from 'react'
import { ChevronDown, ChevronRight } from 'lucide-react'
import { AnimatePresence, motion } from 'framer-motion'
import type { BoardCard } from '@/api/types/BoardCard'
import type { BoardOutput } from '@/api/types/BoardOutput'
import { cn } from '@/lib/utils'
import { DroppableColumn } from './board'
import type { Column } from './board'

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface Lane {
  /** Epic id, or '' for the "No epic" sentinel lane. */
  key: string
  title: string
  columns: {
    todo: BoardCard[]
    in_progress: BoardCard[]
    review: BoardCard[]
    done: BoardCard[]
  }
}

const COLUMN_KEYS: Column[] = ['todo', 'in_progress', 'review', 'done']
const COLUMN_LABELS: Record<Column, string> = {
  todo: 'Todo',
  in_progress: 'In Progress',
  review: 'Review',
  done: 'Done',
}

// ---------------------------------------------------------------------------
// Pure grouping function
// ---------------------------------------------------------------------------

/**
 * Partition the cards in `out` into lanes by epic.  Empty epic lanes are
 * omitted; the "No epic" lane is only appended when orphan cards exist.
 */
export function groupByEpic(out: BoardOutput): Lane[] {
  const lanes: Lane[] = []

  const knownEpicIds = new Set(out.epics.map((e) => e.id))

  for (const epic of out.epics) {
    const lane: Lane = {
      key: epic.id,
      title: epic.title,
      columns: { todo: [], in_progress: [], review: [], done: [] },
    }
    for (const col of COLUMN_KEYS) {
      lane.columns[col] = (out[col] as BoardCard[]).filter(
        (c) => c.epic_id === epic.id,
      )
    }
    const total =
      lane.columns.todo.length +
      lane.columns.in_progress.length +
      lane.columns.review.length +
      lane.columns.done.length
    if (total > 0) lanes.push(lane)
  }

  // Orphan lane — cards with epic_id == null / undefined OR an unknown epic id
  const orphan: Lane = {
    key: '',
    title: 'No epic',
    columns: { todo: [], in_progress: [], review: [], done: [] },
  }
  for (const col of COLUMN_KEYS) {
    orphan.columns[col] = (out[col] as BoardCard[]).filter(
      (c) => c.epic_id == null || !knownEpicIds.has(c.epic_id),
    )
  }
  const orphanTotal =
    orphan.columns.todo.length +
    orphan.columns.in_progress.length +
    orphan.columns.review.length +
    orphan.columns.done.length
  if (orphanTotal > 0) lanes.push(orphan)

  return lanes
}

// ---------------------------------------------------------------------------
// <BoardSwimlanes> component
// ---------------------------------------------------------------------------

interface BoardSwimlanesProps {
  out: BoardOutput
  activeDrag: { id: string; from: Column } | null
  epicMap: Map<string, string>
}

export function BoardSwimlanes({ out, activeDrag, epicMap }: BoardSwimlanesProps) {
  const lanes = groupByEpic(out)

  return (
    <div className="flex flex-col gap-4">
      {lanes.map((lane) => (
        <SwimLane
          key={lane.key === '' ? '__no_epic__' : lane.key}
          lane={lane}
          activeDrag={activeDrag}
          epicMap={epicMap}
        />
      ))}
    </div>
  )
}

// ---------------------------------------------------------------------------
// Individual lane row
// ---------------------------------------------------------------------------

interface SwimLaneProps {
  lane: Lane
  activeDrag: { id: string; from: Column } | null
  epicMap: Map<string, string>
}

function SwimLane({ lane, activeDrag, epicMap }: SwimLaneProps) {
  const [open, setOpen] = useState(true)

  // Roll-up totals across all columns in this lane
  const totalCards =
    lane.columns.todo.length +
    lane.columns.in_progress.length +
    lane.columns.review.length +
    lane.columns.done.length

  const criteriaMet = [...Object.values(lane.columns)]
    .flat()
    .reduce((acc, c) => acc + c.criteria_met, 0)
  const criteriaTotal = [...Object.values(lane.columns)]
    .flat()
    .reduce((acc, c) => acc + c.criteria_total, 0)

  return (
    <div className="rounded-xl border border-border/60 bg-surface-1/30">
      {/* Lane header */}
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        aria-expanded={open}
        className={cn(
          'flex w-full items-center gap-2 px-4 py-2.5 text-sm font-medium hover:bg-surface-2/50 transition-colors',
          'rounded-xl',
          open && 'rounded-b-none border-b border-border/40',
        )}
      >
        {open ? (
          <ChevronDown className="h-4 w-4 shrink-0 text-muted-foreground" />
        ) : (
          <ChevronRight className="h-4 w-4 shrink-0 text-muted-foreground" />
        )}
        <span className="flex-1 truncate text-left">
          {lane.title}
        </span>
        <span className="rounded-full bg-primary/15 px-2 py-0.5 font-mono text-[0.625rem] font-semibold text-primary">
          {totalCards}
        </span>
        {criteriaTotal > 0 && (
          <span className="rounded-sm bg-foreground/8 px-2 py-0.5 font-mono text-[0.625rem] text-muted-foreground">
            {criteriaMet}/{criteriaTotal} AC
          </span>
        )}
      </button>

      {/* Collapsible columns grid */}
      <AnimatePresence initial={false}>
        {open && (
          <motion.div
            key="body"
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: 'auto', opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            transition={{ duration: 0.18, ease: 'easeInOut' }}
            className="overflow-hidden"
          >
            <div className="grid grid-cols-1 gap-4 p-3 md:grid-cols-2 xl:grid-cols-4">
              {COLUMN_KEYS.map((col) => (
                <DroppableColumn
                  key={col}
                  column={col}
                  label={COLUMN_LABELS[col]}
                  cards={lane.columns[col]}
                  activeDrag={activeDrag}
                  epicMap={epicMap}
                  droppableId={`${lane.key}::${col}`}
                />
              ))}
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  )
}

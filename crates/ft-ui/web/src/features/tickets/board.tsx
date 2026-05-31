/**
 * The kanban board. Four columns (todo / in_progress / review / done) driven
 * by `useBoardQuery`; drag-to-column issues an optimistic `useMoveCard`
 * mutation.
 *
 * Click-to-open routes to `/tickets/:id`, which renders the detail drawer on
 * top of `/` via TanStack Router nested routes.
 */
import { useState, useEffect } from 'react'
import {
  DndContext,
  PointerSensor,
  useDraggable,
  useDroppable,
  useSensor,
  useSensors,
  type DragEndEvent,
} from '@dnd-kit/core'
import { Plus, KanbanSquare, Layers } from 'lucide-react'
import { AnimatePresence, motion, useReducedMotion } from 'framer-motion'
import { LAYOUT_TRANSITION, reducedTransition } from '@/lib/motion'
import { EmptyState as SharedEmptyState } from '@/components/ui/empty-state'
import type { BoardCard } from '@/api/types/BoardCard'
import type { BoardOutput } from '@/api/types/BoardOutput'
import { Button } from '@/components/ui/button'
import { Skeleton } from '@/components/ui/skeleton'
import { PageHeader } from '@/components/page-header'
import { cn } from '@/lib/utils'
import { useBoardQuery } from './use-board-query'
import { columnForStatus, useMoveCard } from './use-ticket-mutations'
import { BoardCardBody } from './board-card'
import type { BoardEpic } from '@/api/types/BoardEpic'
import { EpicChips } from './epic-chips'
import { BoardSwimlanes } from './board-swimlanes'

export type Column = keyof Omit<BoardOutput, 'epics'>

const GROUPING_KEY = 'ft-ui:board-group-by-epic'

function readGrouping(): boolean {
  if (typeof window === 'undefined') return false
  try {
    return window.localStorage.getItem(GROUPING_KEY) === '1'
  } catch {
    return false
  }
}

const COLUMNS: Array<{ key: Column; label: string }> = [
  { key: 'todo', label: 'Todo' },
  { key: 'in_progress', label: 'In Progress' },
  { key: 'review', label: 'Review' },
  { key: 'done', label: 'Done' },
]

function buildEpicMap(epics: BoardEpic[]): Map<string, string> {
  return new Map(epics.map((e) => [e.id, e.title]))
}

interface BoardProps {
  onCreateClick: () => void
  ready?: boolean
  onReadyChange?: (next: boolean) => void
  /** Controlled epic filter. When omitted, the board keeps it in local state. */
  epicFilter?: Set<string>
  onEpicFilterChange?: (next: Set<string>) => void
}

export function Board({
  onCreateClick,
  ready = false,
  onReadyChange,
  epicFilter: epicFilterProp,
  onEpicFilterChange,
}: BoardProps) {
  const { data, isLoading, error } = useBoardQuery({ ready })
  const move = useMoveCard()
  const [activeDrag, setActiveDrag] = useState<{ id: string; from: Column } | null>(null)
  const [localEpicFilter, setLocalEpicFilter] = useState<Set<string>>(new Set())
  const epicFilter = epicFilterProp ?? localEpicFilter
  const setEpicFilter = onEpicFilterChange ?? setLocalEpicFilter
  const [groupByEpicOn, setGroupByEpicOn] = useState(readGrouping)
  const sensors = useSensors(useSensor(PointerSensor, { activationConstraint: { distance: 4 } }))

  useEffect(() => {
    try {
      window.localStorage.setItem(GROUPING_KEY, groupByEpicOn ? '1' : '0')
    } catch {
      /* ignore persistence failures (private mode, etc.) */
    }
  }, [groupByEpicOn])

  if (isLoading) return <BoardSkeleton />
  if (error) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-destructive">
        Failed to load board: {(error as Error).message}
      </div>
    )
  }
  if (!data) return null

  const epicMap = buildEpicMap(data.epics ?? [])
  const totalCards = data.todo.length + data.in_progress.length + data.review.length + data.done.length
  if (totalCards === 0) {
    return <EmptyState onCreateClick={onCreateClick} />
  }

  function filterCards(cards: BoardCard[]): BoardCard[] {
    if (epicFilter.size === 0) return cards
    return cards.filter((c) => epicFilter.has(c.epic_id ?? ''))
  }

  function handleDragEnd(e: DragEndEvent) {
    setActiveDrag(null)
    const rawId = e.over?.id as string | undefined
    const drag = e.active.data.current as { id: string; from: Column } | undefined
    if (!rawId || !drag) return
    // In swimlane mode the droppable id is `"${laneKey}::${column}"`.
    // In flat mode it is just the column key. Parse accordingly.
    const overCol = (rawId.includes('::') ? rawId.split('::').pop() : rawId) as Column
    if (!overCol) return
    if (overCol === drag.from) return
    move.mutate({ id: drag.id, from: drag.from, to: overCol })
  }

  return (
    <DndContext
      sensors={sensors}
      onDragStart={(e) => {
        const drag = e.active.data.current as { id: string; from: Column } | undefined
        if (drag) setActiveDrag(drag)
      }}
      onDragCancel={() => setActiveDrag(null)}
      onDragEnd={handleDragEnd}
    >
      <div className="flex h-full flex-col gap-5 px-6 py-5">
        <PageHeader
          title="Board"
          subtitle={`${totalCards} ${totalCards === 1 ? 'ticket' : 'tickets'}`}
          actions={
            <>
              {onReadyChange && (
                <Button
                  size="sm"
                  variant={ready ? 'default' : 'outline'}
                  onClick={() => onReadyChange(!ready)}
                  aria-pressed={ready}
                  data-testid="ready-toggle"
                  className="gap-2"
                >
                  {ready ? 'Unblocked only' : 'Show all'}
                </Button>
              )}
              <Button
                size="sm"
                variant={groupByEpicOn ? 'default' : 'outline'}
                onClick={() => setGroupByEpicOn((v) => !v)}
                aria-pressed={groupByEpicOn}
                data-testid="group-by-epic-toggle"
                className="gap-2"
              >
                <Layers className="h-4 w-4" />
                Group by epic
              </Button>
              <Button onClick={onCreateClick} size="sm" className="gap-2">
                <Plus className="h-4 w-4" />
                New ticket
              </Button>
            </>
          }
        />
        <EpicChips
          epics={data.epics ?? []}
          selected={epicFilter}
          onChange={setEpicFilter}
        />
        {groupByEpicOn ? (
          <BoardSwimlanes
            out={{
              ...data,
              todo: filterCards(data.todo),
              in_progress: filterCards(data.in_progress),
              review: filterCards(data.review),
              done: filterCards(data.done),
            }}
            activeDrag={activeDrag}
            epicMap={epicMap}
          />
        ) : (
          <div className="grid flex-1 grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-4">
            {COLUMNS.map(({ key, label }) => (
              <DroppableColumn
                key={key}
                column={key}
                label={label}
                cards={filterCards(data[key])}
                activeDrag={activeDrag}
                epicMap={epicMap}
              />
            ))}
          </div>
        )}
      </div>
    </DndContext>
  )
}

export interface DroppableColumnProps {
  column: Column
  label: string
  cards: BoardCard[]
  activeDrag: { id: string; from: Column } | null
  epicMap: Map<string, string>
  /** Override the droppable id (defaults to `column`). Use composite ids in swimlane mode. */
  droppableId?: string
}

export function DroppableColumn({ column, label, cards, activeDrag, epicMap, droppableId }: DroppableColumnProps) {
  const { setNodeRef, isOver } = useDroppable({ id: droppableId ?? column })
  return (
    <div
      ref={setNodeRef}
      data-testid={`column-${column}`}
      className={cn(
        'flex flex-col gap-2.5 rounded-xl border border-border/60 bg-surface-1/50 p-2.5 transition-colors',
        isOver && 'border-primary/60 bg-primary/5',
      )}
    >
      <div className="flex items-center justify-between px-1.5 pb-0.5 pt-1">
        <span className="text-sm font-medium uppercase tracking-wide text-muted-foreground">
          {label}
        </span>
        <span className="rounded-full bg-primary/15 px-2 py-0.5 font-mono text-[0.625rem] font-semibold text-primary">
          {cards.length}
        </span>
      </div>
      <div className="flex flex-1 flex-col gap-2.5 overflow-y-auto">
        <AnimatePresence initial={false}>
          {cards.map((card) => (
            <DraggableCard
              key={card.id}
              card={card}
              column={column}
              dragging={activeDrag?.id === card.id}
              epicMap={epicMap}
            />
          ))}
        </AnimatePresence>
      </div>
    </div>
  )
}

export interface DraggableCardProps {
  card: BoardCard
  column: Column
  dragging: boolean
  epicMap: Map<string, string>
}

export function DraggableCard({ card, column, dragging, epicMap }: DraggableCardProps) {
  const { attributes, listeners, setNodeRef, transform } = useDraggable({
    id: card.id,
    data: { id: card.id, from: column },
  })
  const reduced = useReducedMotion() ?? false
  const transition = reducedTransition(reduced, LAYOUT_TRANSITION)
  const style = transform
    ? { transform: `translate3d(${transform.x}px, ${transform.y}px, 0)` }
    : undefined
  const active = column === 'in_progress'
  const epicTitle = card.epic_id ? epicMap.get(card.epic_id) : undefined
  return (
    <motion.div
      ref={setNodeRef}
      style={style}
      layout={!dragging && !reduced ? 'position' : false}
      initial={reduced ? false : { opacity: 0, y: 4 }}
      animate={{ opacity: 1, y: 0 }}
      exit={reduced ? { opacity: 0 } : { opacity: 0, y: -4 }}
      transition={transition}
      {...listeners}
      {...attributes}
      data-testid={`card-${card.id}`}
      className={cn(
        'group flex cursor-grab flex-col gap-2.5 rounded-lg border border-border bg-card p-3 text-left text-card-foreground shadow-elevation-1 transition-colors',
        'hover:bg-surface-2 hover:border-primary/40 focus-within:ring-2 focus-within:ring-ring',
        active && 'ring-1 ring-primary/25 shadow-glow',
        dragging && 'opacity-40',
      )}
    >
      <BoardCardBody card={card} epicTitle={epicTitle} />
    </motion.div>
  )
}

export function PriorityBadge({ priority }: { priority: string }) {
  const tone =
    priority === 'p0' || priority === 'p1'
      ? 'bg-primary/20 text-primary'
      : priority === 'p2'
        ? 'bg-foreground/10 text-foreground'
        : 'bg-muted text-muted-foreground'
  return (
    <span
      className={cn(
        'rounded-sm px-1.5 py-0.5 font-mono text-[0.625rem] font-semibold uppercase tracking-wider',
        tone,
      )}
    >
      {priority}
    </span>
  )
}

function BoardSkeleton() {
  return (
    <div className="flex h-full flex-col gap-5 px-6 py-5">
      <div className="flex items-center justify-between">
        <Skeleton className="h-7 w-28" />
        <Skeleton className="h-8 w-32" />
      </div>
      <div className="grid flex-1 grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-4">
        {COLUMNS.map((c) => (
          <div
            key={c.key}
            className="flex flex-col gap-2.5 rounded-xl border border-border/60 bg-surface-1/50 p-2.5"
          >
            <div className="flex items-center justify-between px-1.5 pb-0.5 pt-1">
              <Skeleton className="h-4 w-20" />
              <Skeleton className="h-4 w-6 rounded-full" />
            </div>
            <Skeleton className="h-[4.5rem] w-full rounded-lg" />
            <Skeleton className="h-[4.5rem] w-full rounded-lg" />
          </div>
        ))}
      </div>
    </div>
  )
}

function EmptyState({ onCreateClick }: { onCreateClick: () => void }) {
  return (
    <div className="flex h-full items-center justify-center p-8">
      <SharedEmptyState
        icon={KanbanSquare}
        title="No tickets yet"
        description="File the first one — epics, tasks, subtasks, and bugs all live on the board."
        action={
          <Button onClick={onCreateClick} className="gap-2">
            <Plus className="h-4 w-4" />
            Create ticket
          </Button>
        }
      />
    </div>
  )
}

export { columnForStatus }

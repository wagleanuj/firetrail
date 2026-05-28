/**
 * The kanban board. Four columns (todo / in_progress / review / done) driven
 * by `useBoardQuery`; drag-to-column issues an optimistic `useMoveCard`
 * mutation.
 *
 * Click-to-open routes to `/tickets/:id`, which renders the detail drawer on
 * top of `/` via TanStack Router nested routes.
 */
import { useState } from 'react'
import { Link } from '@tanstack/react-router'
import {
  DndContext,
  PointerSensor,
  useDraggable,
  useDroppable,
  useSensor,
  useSensors,
  type DragEndEvent,
} from '@dnd-kit/core'
import { Plus } from 'lucide-react'
import type { BoardCard } from '@/api/types/BoardCard'
import type { BoardOutput } from '@/api/types/BoardOutput'
import { Button } from '@/components/ui/button'
import { Skeleton } from '@/components/ui/skeleton'
import { cn } from '@/lib/utils'
import { useBoardQuery } from './use-board-query'
import { columnForStatus, useMoveCard } from './use-ticket-mutations'

type Column = keyof BoardOutput

const COLUMNS: Array<{ key: Column; label: string }> = [
  { key: 'todo', label: 'Todo' },
  { key: 'in_progress', label: 'In Progress' },
  { key: 'review', label: 'Review' },
  { key: 'done', label: 'Done' },
]

interface BoardProps {
  onCreateClick: () => void
}

export function Board({ onCreateClick }: BoardProps) {
  const { data, isLoading, error } = useBoardQuery()
  const move = useMoveCard()
  const [activeDrag, setActiveDrag] = useState<{ id: string; from: Column } | null>(null)
  const sensors = useSensors(useSensor(PointerSensor, { activationConstraint: { distance: 4 } }))

  if (isLoading) return <BoardSkeleton />
  if (error) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-destructive">
        Failed to load board: {(error as Error).message}
      </div>
    )
  }
  if (!data) return null

  const totalCards = data.todo.length + data.in_progress.length + data.review.length + data.done.length
  if (totalCards === 0) {
    return <EmptyState onCreateClick={onCreateClick} />
  }

  function handleDragEnd(e: DragEndEvent) {
    setActiveDrag(null)
    const overCol = e.over?.id as Column | undefined
    const drag = e.active.data.current as { id: string; from: Column } | undefined
    if (!overCol || !drag) return
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
      <div className="flex h-full flex-col gap-4 p-4">
        <div className="flex items-center justify-between">
          <h1 className="font-mono text-lg font-semibold tracking-tight">Board</h1>
          <Button onClick={onCreateClick} size="sm" className="gap-2">
            <Plus className="h-4 w-4" />
            New ticket
          </Button>
        </div>
        <div className="grid flex-1 grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-4">
          {COLUMNS.map(({ key, label }) => (
            <DroppableColumn
              key={key}
              column={key}
              label={label}
              cards={data[key]}
              activeDrag={activeDrag}
            />
          ))}
        </div>
      </div>
    </DndContext>
  )
}

interface DroppableColumnProps {
  column: Column
  label: string
  cards: BoardCard[]
  activeDrag: { id: string; from: Column } | null
}

function DroppableColumn({ column, label, cards, activeDrag }: DroppableColumnProps) {
  const { setNodeRef, isOver } = useDroppable({ id: column })
  return (
    <div
      ref={setNodeRef}
      data-testid={`column-${column}`}
      className={cn(
        'flex flex-col gap-2 rounded-lg border border-border/50 bg-card/40 p-2 transition-colors',
        isOver && 'border-primary/60 bg-primary/5',
      )}
    >
      <div className="flex items-center justify-between px-2 py-1">
        <span className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
          {label}
        </span>
        <span className="rounded-full bg-primary/15 px-2 py-0.5 font-mono text-[0.625rem] font-semibold text-primary">
          {cards.length}
        </span>
      </div>
      <div className="flex flex-1 flex-col gap-2 overflow-y-auto">
        {cards.map((card) => (
          <DraggableCard
            key={card.id}
            card={card}
            column={column}
            dragging={activeDrag?.id === card.id}
          />
        ))}
      </div>
    </div>
  )
}

interface DraggableCardProps {
  card: BoardCard
  column: Column
  dragging: boolean
}

function DraggableCard({ card, column, dragging }: DraggableCardProps) {
  const { attributes, listeners, setNodeRef, transform } = useDraggable({
    id: card.id,
    data: { id: card.id, from: column },
  })
  const style = transform
    ? { transform: `translate3d(${transform.x}px, ${transform.y}px, 0)` }
    : undefined
  return (
    <div
      ref={setNodeRef}
      style={style}
      {...listeners}
      {...attributes}
      data-testid={`card-${card.id}`}
      className={cn(
        'group cursor-grab rounded-md border border-border/70 bg-background/80 p-3 text-left shadow-sm transition-all',
        'hover:-translate-y-0.5 hover:border-primary/40 hover:shadow-[0_0_0_1px_hsl(var(--primary)/0.25)] focus-within:ring-2 focus-within:ring-ring',
        dragging && 'opacity-40',
      )}
    >
      <div className="mb-1 flex items-center justify-between gap-2">
        <Link
          to="/tickets/$id"
          params={{ id: card.id }}
          className="truncate font-mono text-[0.65rem] uppercase tracking-wider text-muted-foreground hover:text-primary"
          onPointerDown={(e) => e.stopPropagation()}
        >
          {card.short_id}
        </Link>
        <PriorityBadge priority={card.priority} />
      </div>
      <Link
        to="/tickets/$id"
        params={{ id: card.id }}
        className="block text-sm leading-snug text-foreground hover:text-primary"
        onPointerDown={(e) => e.stopPropagation()}
      >
        {card.title}
      </Link>
      {card.owner && (
        <div className="mt-2 flex items-center gap-1.5 text-xs text-muted-foreground">
          <Avatar name={card.owner} />
          <span className="truncate">{card.owner}</span>
        </div>
      )}
    </div>
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

function Avatar({ name }: { name: string }) {
  const initial = name.trim().charAt(0).toUpperCase() || '?'
  return (
    <span className="flex h-5 w-5 items-center justify-center rounded-full bg-primary/20 font-mono text-[0.65rem] font-semibold text-primary">
      {initial}
    </span>
  )
}

function BoardSkeleton() {
  return (
    <div className="grid h-full grid-cols-1 gap-3 p-4 md:grid-cols-2 xl:grid-cols-4">
      {COLUMNS.map((c) => (
        <div key={c.key} className="rounded-lg border border-border/50 bg-card/40 p-2">
          <Skeleton className="mb-3 h-4 w-24" />
          <Skeleton className="mb-2 h-16 w-full" />
          <Skeleton className="mb-2 h-16 w-full" />
        </div>
      ))}
    </div>
  )
}

function EmptyState({ onCreateClick }: { onCreateClick: () => void }) {
  return (
    <div className="flex h-full items-center justify-center p-8">
      <div className="max-w-md rounded-xl border border-border/70 bg-card/50 p-8 text-center shadow-[0_0_0_1px_hsl(var(--border)/0.4)_inset]">
        <h2 className="font-mono text-2xl font-semibold">No tickets yet</h2>
        <p className="mt-2 text-sm text-muted-foreground">
          Click the button below to file the first one. Daily-driver UI begins
          here.
        </p>
        <Button onClick={onCreateClick} className="mt-6 gap-2">
          <Plus className="h-4 w-4 text-primary-foreground" />
          Create ticket
        </Button>
      </div>
    </div>
  )
}

export { columnForStatus }

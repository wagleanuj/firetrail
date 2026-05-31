/**
 * Backlog table view — dense, sortable, filterable list of all tickets across
 * all board columns. Reuses `useBoardQuery` (no new endpoint).
 */
import * as React from 'react'
import { Link } from '@tanstack/react-router'
import { ListTodo, ArrowUpDown, ArrowUp, ArrowDown } from 'lucide-react'
import { Badge, type BadgeProps } from '@/components/ui/badge'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Skeleton } from '@/components/ui/skeleton'
import { EmptyState } from '@/components/ui/empty-state'
import { PageHeader } from '@/components/page-header'
import { cn } from '@/lib/utils'
import { useBoardQuery } from './use-board-query'
import { useUpdateTicket } from './use-ticket-mutations'
import type { BoardCard } from '@/api/types/BoardCard'

// ─── Types ────────────────────────────────────────────────────────────────────

export interface BacklogRow extends BoardCard {
  /** The board column this card came from. */
  status: 'todo' | 'in_progress' | 'review' | 'done'
  subtask_count: number
  blocked_by_count: number
}

export type SortKey = 'priority' | 'title' | 'status' | 'kind' | 'owner'

// ─── Pure helpers ─────────────────────────────────────────────────────────────

const PRIORITY_ORDER: Record<string, number> = {
  p0: 0,
  p1: 1,
  p2: 2,
  p3: 3,
  p4: 4,
}

const STATUS_ORDER: Record<string, number> = {
  todo: 0,
  in_progress: 1,
  review: 2,
  done: 3,
}

export function sortRows(
  rows: BacklogRow[],
  key: SortKey,
  dir: 'asc' | 'desc',
): BacklogRow[] {
  const sign = dir === 'asc' ? 1 : -1
  return [...rows].sort((a, b) => {
    let cmp = 0
    if (key === 'priority') {
      cmp = (PRIORITY_ORDER[a.priority] ?? 99) - (PRIORITY_ORDER[b.priority] ?? 99)
    } else if (key === 'status') {
      cmp = (STATUS_ORDER[a.status] ?? 99) - (STATUS_ORDER[b.status] ?? 99)
    } else if (key === 'title') {
      cmp = a.title.localeCompare(b.title)
    } else if (key === 'kind') {
      cmp = a.kind.localeCompare(b.kind)
    } else if (key === 'owner') {
      cmp = (a.owner ?? '').localeCompare(b.owner ?? '')
    }
    return cmp * sign
  })
}

// ─── Kind pill mapping ────────────────────────────────────────────────────────

const KIND_VARIANT: Record<string, BadgeProps['variant']> = {
  epic: 'epic',
  task: 'task',
  subtask: 'task',
  bug: 'bug',
  feature: 'feature',
}

// ─── Flatten board output ─────────────────────────────────────────────────────

function flattenBoard(data: {
  todo: BoardCard[]
  in_progress: BoardCard[]
  review: BoardCard[]
  done: BoardCard[]
}): BacklogRow[] {
  const pairs: [BacklogRow['status'], BoardCard[]][] = [
    ['todo', data.todo],
    ['in_progress', data.in_progress],
    ['review', data.review],
    ['done', data.done],
  ]
  const result: BacklogRow[] = []
  for (const [status, cards] of pairs) {
    for (const card of cards) {
      result.push({ ...card, status } as BacklogRow)
    }
  }
  return result
}

// ─── Sub-components ───────────────────────────────────────────────────────────

function SortIcon({
  col,
  active,
  dir,
}: {
  col: SortKey
  active: SortKey
  dir: 'asc' | 'desc'
}) {
  if (col !== active) return <ArrowUpDown className="ml-1 inline h-3 w-3 opacity-40" />
  return dir === 'asc'
    ? <ArrowUp className="ml-1 inline h-3 w-3 text-primary" />
    : <ArrowDown className="ml-1 inline h-3 w-3 text-primary" />
}

function SortableHead({
  col,
  label,
  sortKey,
  sortDir,
  onSort,
  className,
}: {
  col: SortKey
  label: string
  sortKey: SortKey
  sortDir: 'asc' | 'desc'
  onSort: (col: SortKey) => void
  className?: string
}) {
  return (
    <TableHead
      className={cn('cursor-pointer select-none whitespace-nowrap hover:text-foreground', className)}
      onClick={() => onSort(col)}
    >
      {label}
      <SortIcon col={col} active={sortKey} dir={sortDir} />
    </TableHead>
  )
}

/** Compact inline priority editor. */
function PriorityCell({ row }: { row: BacklogRow }) {
  const update = useUpdateTicket(row.id)
  return (
    <Select
      value={row.priority}
      onValueChange={(val) =>
        update.mutate({ priority: val as 'p0' | 'p1' | 'p2' | 'p3' | 'p4' })
      }
    >
      <SelectTrigger
        className="h-6 w-16 border-0 bg-transparent px-1 py-0 font-mono text-[0.625rem] shadow-none focus:ring-0"
        aria-label={`Priority for ${row.short_id}`}
      >
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        {(['p0', 'p1', 'p2', 'p3', 'p4'] as const).map((p) => (
          <SelectItem key={p} value={p} className="font-mono text-xs">
            {p}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  )
}

/** Compact inline status editor. */
function StatusCell({ row }: { row: BacklogRow }) {
  const update = useUpdateTicket(row.id)
  const STATUS_OPTS = [
    { value: 'open', label: 'Open' },
    { value: 'ready', label: 'Ready' },
    { value: 'in_progress', label: 'In Progress' },
    { value: 'review', label: 'Review' },
    { value: 'closed', label: 'Closed' },
  ]
  // Derive display label from column
  const colToStatus: Record<BacklogRow['status'], string> = {
    todo: 'open',
    in_progress: 'in_progress',
    review: 'review',
    done: 'closed',
  }
  const currentStatus = colToStatus[row.status]
  return (
    <Select
      value={currentStatus}
      onValueChange={(val) =>
        update.mutate({
          status: val as 'open' | 'ready' | 'in_progress' | 'review' | 'closed',
        })
      }
    >
      <SelectTrigger
        className="h-6 w-28 border-0 bg-transparent px-1 py-0 text-xs shadow-none focus:ring-0"
        aria-label={`Status for ${row.short_id}`}
      >
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        {STATUS_OPTS.map(({ value, label }) => (
          <SelectItem key={value} value={value} className="text-xs">
            {label}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  )
}

/** Filter bar above the table. */
function FilterBar({
  rows,
  epicFilter,
  ownerFilter,
  statusFilter,
  kindFilter,
  epicMap,
  onEpicChange,
  onOwnerChange,
  onStatusChange,
  onKindChange,
}: {
  rows: BacklogRow[]
  epicFilter: string
  ownerFilter: string
  statusFilter: string
  kindFilter: string
  epicMap: Map<string, string>
  onEpicChange: (v: string) => void
  onOwnerChange: (v: string) => void
  onStatusChange: (v: string) => void
  onKindChange: (v: string) => void
}) {
  const owners = React.useMemo(
    () => Array.from(new Set(rows.map((r) => r.owner).filter(Boolean) as string[])).sort(),
    [rows],
  )
  const kinds = React.useMemo(
    () => Array.from(new Set(rows.map((r) => r.kind))).sort(),
    [rows],
  )
  const epics = React.useMemo(() => Array.from(epicMap.entries()), [epicMap])

  return (
    <div className="flex flex-wrap items-center gap-2">
      {/* Epic filter */}
      <Select value={epicFilter} onValueChange={onEpicChange}>
        <SelectTrigger className="h-8 w-40 text-xs" aria-label="Filter by epic">
          <SelectValue placeholder="All epics" />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="__all__" className="text-xs">All epics</SelectItem>
          {epics.map(([id, title]) => (
            <SelectItem key={id} value={id} className="text-xs truncate max-w-[14rem]">
              {title}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>

      {/* Owner filter */}
      <Select value={ownerFilter} onValueChange={onOwnerChange}>
        <SelectTrigger className="h-8 w-36 text-xs" aria-label="Filter by owner">
          <SelectValue placeholder="All owners" />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="__all__" className="text-xs">All owners</SelectItem>
          {owners.map((o) => (
            <SelectItem key={o} value={o} className="text-xs">
              {o}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>

      {/* Status filter */}
      <Select value={statusFilter} onValueChange={onStatusChange}>
        <SelectTrigger className="h-8 w-36 text-xs" aria-label="Filter by status">
          <SelectValue placeholder="All statuses" />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="__all__" className="text-xs">All statuses</SelectItem>
          <SelectItem value="todo" className="text-xs">Todo</SelectItem>
          <SelectItem value="in_progress" className="text-xs">In Progress</SelectItem>
          <SelectItem value="review" className="text-xs">Review</SelectItem>
          <SelectItem value="done" className="text-xs">Done</SelectItem>
        </SelectContent>
      </Select>

      {/* Kind filter */}
      <Select value={kindFilter} onValueChange={onKindChange}>
        <SelectTrigger className="h-8 w-32 text-xs" aria-label="Filter by kind">
          <SelectValue placeholder="All kinds" />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="__all__" className="text-xs">All kinds</SelectItem>
          {kinds.map((k) => (
            <SelectItem key={k} value={k} className="text-xs capitalize">
              {k}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </div>
  )
}

// ─── Backlog skeleton ─────────────────────────────────────────────────────────

function BacklogSkeleton() {
  return (
    <div className="flex flex-col gap-5 px-6 py-5">
      <div className="flex items-center justify-between">
        <Skeleton className="h-7 w-28" />
        <Skeleton className="h-8 w-40" />
      </div>
      <div className="flex gap-2">
        {[140, 140, 140, 120].map((w, i) => (
          <Skeleton key={i} className="h-8" style={{ width: w }} />
        ))}
      </div>
      <div className="rounded-xl border border-border/60">
        {Array.from({ length: 8 }).map((_, i) => (
          <div key={i} className="flex items-center gap-4 border-b border-border/40 px-3 py-2">
            <Skeleton className="h-4 w-16" />
            <Skeleton className="h-4 w-20" />
            <Skeleton className="h-4 flex-1" />
            <Skeleton className="h-4 w-16" />
            <Skeleton className="h-4 w-16" />
          </div>
        ))}
      </div>
    </div>
  )
}

// ─── Main component ───────────────────────────────────────────────────────────

export function Backlog() {
  const { data, isLoading, error } = useBoardQuery({})

  const [sortKey, setSortKey] = React.useState<SortKey>('priority')
  const [sortDir, setSortDir] = React.useState<'asc' | 'desc'>('asc')
  const [epicFilter, setEpicFilter] = React.useState('__all__')
  const [ownerFilter, setOwnerFilter] = React.useState('__all__')
  const [statusFilter, setStatusFilter] = React.useState('__all__')
  const [kindFilter, setKindFilter] = React.useState('__all__')

  function handleSort(col: SortKey) {
    if (col === sortKey) {
      setSortDir((d) => (d === 'asc' ? 'desc' : 'asc'))
    } else {
      setSortKey(col)
      setSortDir('asc')
    }
  }

  const epicMap = React.useMemo(
    () => new Map((data?.epics ?? []).map((e) => [e.id, e.title])),
    [data],
  )

  const allRows = React.useMemo(() => (data ? flattenBoard(data) : []), [data])

  const filtered = React.useMemo(() => {
    let rows = allRows
    if (epicFilter !== '__all__') rows = rows.filter((r) => r.epic_id === epicFilter)
    if (ownerFilter !== '__all__') rows = rows.filter((r) => r.owner === ownerFilter)
    if (statusFilter !== '__all__') rows = rows.filter((r) => r.status === statusFilter)
    if (kindFilter !== '__all__') rows = rows.filter((r) => r.kind === kindFilter)
    return rows
  }, [allRows, epicFilter, ownerFilter, statusFilter, kindFilter])

  const sorted = React.useMemo(
    () => sortRows(filtered, sortKey, sortDir),
    [filtered, sortKey, sortDir],
  )

  if (isLoading) return <BacklogSkeleton />
  if (error) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-destructive">
        Failed to load backlog: {(error as Error).message}
      </div>
    )
  }
  if (!data) return null

  const sharedSortProps = { sortKey, sortDir, onSort: handleSort }

  return (
    <div className="flex h-full flex-col gap-5 px-6 py-5">
      <PageHeader
        title="Backlog"
        subtitle={`${sorted.length} of ${allRows.length} tickets`}
      />

      <FilterBar
        rows={allRows}
        epicFilter={epicFilter}
        ownerFilter={ownerFilter}
        statusFilter={statusFilter}
        kindFilter={kindFilter}
        epicMap={epicMap}
        onEpicChange={setEpicFilter}
        onOwnerChange={setOwnerFilter}
        onStatusChange={setStatusFilter}
        onKindChange={setKindFilter}
      />

      {sorted.length === 0 ? (
        <div className="flex flex-1 items-center justify-center">
          <EmptyState
            icon={ListTodo}
            title="No tickets match"
            description="Try adjusting the filters above."
          />
        </div>
      ) : (
        <div className="flex-1 overflow-auto rounded-xl border border-border/60">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead className="w-16">Kind</TableHead>
                <TableHead className="w-24 font-mono">ID</TableHead>
                <SortableHead col="title" label="Title" {...sharedSortProps} className="min-w-[12rem]" />
                <TableHead className="w-40">Epic</TableHead>
                <SortableHead col="priority" label="Priority" {...sharedSortProps} className="w-24" />
                <SortableHead col="status" label="Status" {...sharedSortProps} className="w-32" />
                <SortableHead col="owner" label="Owner" {...sharedSortProps} className="w-32" />
                <TableHead className="w-20 text-right">Criteria</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {sorted.map((row) => (
                <BacklogTableRow key={row.id} row={row} epicMap={epicMap} />
              ))}
            </TableBody>
          </Table>
        </div>
      )}
    </div>
  )
}

function BacklogTableRow({
  row,
  epicMap,
}: {
  row: BacklogRow
  epicMap: Map<string, string>
}) {
  const variant = KIND_VARIANT[row.kind] ?? 'secondary'
  const epicTitle = row.epic_id ? epicMap.get(row.epic_id) : undefined

  return (
    <TableRow>
      {/* Kind pill */}
      <TableCell>
        <Badge variant={variant} className="px-1.5 py-0 text-[0.625rem] capitalize">
          {row.kind}
        </Badge>
      </TableCell>

      {/* Short ID */}
      <TableCell className="font-mono text-xs text-muted-foreground">
        {row.short_id}
      </TableCell>

      {/* Title — navigates to detail */}
      <TableCell>
        <Link
          to="/tickets/$id"
          params={{ id: row.id }}
          className="text-sm font-medium text-foreground hover:text-primary"
        >
          {row.title}
        </Link>
        {row.blocked_by_count > 0 && (
          <span className="ml-2 rounded-full bg-destructive/15 px-1.5 py-0.5 text-[0.625rem] text-destructive">
            blocked
          </span>
        )}
      </TableCell>

      {/* Epic */}
      <TableCell className="max-w-[10rem] truncate text-xs text-muted-foreground">
        {epicTitle ?? <span className="opacity-30">—</span>}
      </TableCell>

      {/* Priority — inline edit */}
      <TableCell className="p-1">
        <PriorityCell row={row} />
      </TableCell>

      {/* Status — inline edit */}
      <TableCell className="p-1">
        <StatusCell row={row} />
      </TableCell>

      {/* Owner */}
      <TableCell className="max-w-[8rem] truncate text-xs text-muted-foreground">
        {row.owner ?? <span className="opacity-30">—</span>}
      </TableCell>

      {/* Criteria */}
      <TableCell className="text-right font-mono text-xs text-muted-foreground">
        {row.criteria_total > 0
          ? `${row.criteria_met}/${row.criteria_total}`
          : <span className="opacity-30">—</span>}
      </TableCell>
    </TableRow>
  )
}

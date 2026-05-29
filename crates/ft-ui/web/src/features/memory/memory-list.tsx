/**
 * Memory browser — a filter sidebar plus a list of cards.
 *
 * Filters live in URL search params (driven by TanStack Router) so they
 * survive reload and are deep-linkable. We render the list as a plain
 * `map` for the first cut: real-world memory volumes are bounded by
 * `?limit=` and don't currently need virtualisation.
 */
import { Link, useNavigate, useSearch } from '@tanstack/react-router'
import { Plus, FileWarning, Database, Filter } from 'lucide-react'
import { AnimatePresence, motion, useReducedMotion } from 'framer-motion'
import { LIST_STAGGER, ROUTE_TRANSITION, reducedTransition } from '@/lib/motion'
import { EmptyState as SharedEmptyState } from '@/components/ui/empty-state'
import type { MemoryKind } from '@/api/types/MemoryKind'
import type { TrustStateInput } from '@/api/types/TrustStateInput'
import type { MemoryRowOut } from '@/api/types/MemoryRowOut'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Skeleton } from '@/components/ui/skeleton'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { PageHeader } from '@/components/page-header'
import { cn } from '@/lib/utils'
import { useMemoryList } from './use-memory-query'
import { MEMORY_KINDS, TRUST_STATES } from './types'
import { TrustBadge, trustRailClass } from '@/features/trust/trust-badge'

export interface MemorySearchParams {
  kind?: MemoryKind
  trust?: TrustStateInput
  stale?: boolean
}

interface MemoryListProps {
  onCreateClick: () => void
}

export function MemoryList({ onCreateClick }: MemoryListProps) {
  const search = useSearch({ from: '/memory/' }) as MemorySearchParams
  const navigate = useNavigate({ from: '/memory/' })
  const filters = {
    kind: search.kind ?? null,
    trust: search.trust ?? null,
    stale: search.stale ?? false,
  }
  const { data, isLoading, error } = useMemoryList(filters)
  const reduced = useReducedMotion() ?? false
  const transition = reducedTransition(reduced, ROUTE_TRANSITION)

  function updateFilters(next: Partial<MemorySearchParams>) {
    const merged: MemorySearchParams = { ...search, ...next }
    const cleaned: MemorySearchParams = {}
    if (merged.kind) cleaned.kind = merged.kind
    if (merged.trust) cleaned.trust = merged.trust
    if (merged.stale) cleaned.stale = true
    navigate({ search: cleaned })
  }

  return (
    <div className="flex h-full flex-col gap-5 px-6 py-5">
      <PageHeader
        title="Memory"
        subtitle={
          <span className="font-mono text-xs text-muted-foreground">
            {data ? `${data.rows.length} record${data.rows.length === 1 ? '' : 's'}` : '—'}
          </span>
        }
        actions={
          <Button onClick={onCreateClick} size="sm" className="gap-2">
            <Plus className="h-4 w-4" />
            New memory
          </Button>
        }
      />

      <div className="grid min-h-0 flex-1 grid-cols-1 gap-6 lg:grid-cols-[14rem_1fr]">
        {/* Filter sidebar */}
        <aside className="space-y-4">
        <h2 className="text-sm font-medium uppercase tracking-wide text-muted-foreground">
          Filters
        </h2>

        <div className="space-y-1.5">
          <Label>Kind</Label>
          <Select
            value={search.kind ?? '__all__'}
            onValueChange={(v) =>
              updateFilters({ kind: v === '__all__' ? undefined : (v as MemoryKind) })
            }
          >
            <SelectTrigger>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="__all__">All kinds</SelectItem>
              {MEMORY_KINDS.map((k) => (
                <SelectItem key={k} value={k}>
                  {k}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>

        <div className="space-y-1.5">
          <Label>Trust</Label>
          <Select
            value={search.trust ?? '__any__'}
            onValueChange={(v) =>
              updateFilters({ trust: v === '__any__' ? undefined : (v as TrustStateInput) })
            }
          >
            <SelectTrigger>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="__any__">Any trust</SelectItem>
              {TRUST_STATES.map((t) => (
                <SelectItem key={t} value={t}>
                  {t}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>

        <label className="flex items-center gap-2 text-sm">
          <Input
            type="checkbox"
            className="h-4 w-4 accent-primary"
            checked={!!search.stale}
            onChange={(e) => updateFilters({ stale: e.target.checked || undefined })}
          />
          <span>Stale only</span>
        </label>
        </aside>

        {/* List */}
        <section className="flex min-h-0 flex-col gap-3">
        {isLoading && <ListSkeleton />}
        {error && (
          <div className="text-sm text-destructive">
            Failed to load memory: {(error as Error).message}
          </div>
        )}
        {data && data.rows.length === 0 && (
          <div className="flex flex-1 items-center justify-center p-8">
            {filtersActive(search) ? (
              <SharedEmptyState
                icon={Filter}
                title="No records match these filters"
                description="Clear the filter sidebar to broaden your view, or create a new memory record."
                action={
                  <Button onClick={onCreateClick} size="sm" className="gap-2">
                    <Plus className="h-4 w-4" />
                    Create memory
                  </Button>
                }
              />
            ) : (
              <SharedEmptyState
                icon={Database}
                title="No memory yet"
                description="Capture the first one — incidents, findings, runbooks, decisions, gotchas, or freeform notes all live here."
                action={
                  <Button onClick={onCreateClick} className="gap-2">
                    <Plus className="h-4 w-4" />
                    Create memory
                  </Button>
                }
              />
            )}
          </div>
        )}
        {data && data.rows.length > 0 && (
          <ul
            data-testid="memory-list"
            className="grid grid-cols-1 gap-2.5 overflow-y-auto md:grid-cols-2"
          >
            <AnimatePresence initial={!reduced}>
              {data.rows.map((row, i) => (
                <motion.li
                  key={row.id}
                  initial={reduced ? false : { opacity: 0, y: 4 }}
                  animate={{ opacity: 1, y: 0 }}
                  exit={reduced ? { opacity: 0 } : { opacity: 0, y: -4 }}
                  transition={{ ...transition, delay: reduced ? 0 : Math.min(i, 12) * LIST_STAGGER }}
                >
                  <MemoryCard row={row} />
                </motion.li>
              ))}
            </AnimatePresence>
          </ul>
        )}
        </section>
      </div>
    </div>
  )
}

export function MemoryCard({ row }: { row: MemoryRowOut }) {
  return (
    <Link
      to="/memory/$id"
      params={{ id: row.id }}
      data-testid={`memory-card-${row.id}`}
      className={cn(
        'group relative block overflow-hidden rounded-lg border border-border bg-card p-3 pl-4 text-left shadow-elevation-1 transition-colors',
        'hover:border-primary/40 hover:bg-surface-2',
      )}
    >
      {/* Trust-tone rail — scannable down a column. */}
      <span
        aria-hidden
        className={cn(
          'absolute inset-y-0 left-0 w-1',
          trustRailClass(row.trust, row.stale ?? false),
        )}
      />
      <div className="mb-1.5 flex items-center justify-between gap-2">
        <KindBadge kind={row.kind} />
        <span className="font-mono text-[0.65rem] uppercase tracking-wider text-muted-foreground">
          {row.id.slice(0, 14)}
        </span>
      </div>
      <div className="text-sm font-medium leading-snug text-foreground">{row.title}</div>
      <div className="mt-2.5 flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
        {row.trust && <TrustBadge state={row.trust} emphasizeStale={row.stale ?? false} />}
        {row.riskClass && (
          <span className="rounded-full bg-muted px-2 py-0.5 font-mono text-[0.625rem] uppercase tracking-wider">
            {row.riskClass}
          </span>
        )}
        {row.stale && (
          <span className="inline-flex items-center gap-1 font-mono text-[0.625rem] uppercase tracking-wider text-danger">
            <FileWarning className="h-3 w-3" /> stale
          </span>
        )}
      </div>
    </Link>
  )
}

export function KindBadge({ kind }: { kind: string }) {
  return (
    <span
      className={cn(
        'rounded-full bg-primary/15 px-2 py-0.5 font-mono text-[0.625rem] font-semibold uppercase tracking-wider text-primary',
      )}
    >
      {kind}
    </span>
  )
}

function ListSkeleton() {
  return (
    <div className="grid grid-cols-1 gap-2.5 md:grid-cols-2">
      {Array.from({ length: 6 }).map((_, i) => (
        <Skeleton key={i} className="h-24 w-full rounded-lg" />
      ))}
    </div>
  )
}

function filtersActive(s: MemorySearchParams): boolean {
  return Boolean(s.kind || s.trust || s.stale)
}

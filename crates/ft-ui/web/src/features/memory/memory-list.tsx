/**
 * Memory browser — a filter sidebar plus a list of cards.
 *
 * Filters live in URL search params (driven by TanStack Router) so they
 * survive reload and are deep-linkable. We render the list as a plain
 * `map` for the first cut: real-world memory volumes are bounded by
 * `?limit=` and don't currently need virtualisation.
 */
import { Link, useNavigate, useSearch } from '@tanstack/react-router'
import { Plus, FileWarning } from 'lucide-react'
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
import { cn } from '@/lib/utils'
import { useMemoryList } from './use-memory-query'
import { MEMORY_KINDS, TRUST_STATES } from './types'

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

  function updateFilters(next: Partial<MemorySearchParams>) {
    const merged: MemorySearchParams = { ...search, ...next }
    const cleaned: MemorySearchParams = {}
    if (merged.kind) cleaned.kind = merged.kind
    if (merged.trust) cleaned.trust = merged.trust
    if (merged.stale) cleaned.stale = true
    navigate({ search: cleaned })
  }

  return (
    <div className="grid h-full grid-cols-1 gap-4 p-4 lg:grid-cols-[14rem_1fr]">
      {/* Filter sidebar */}
      <aside className="space-y-4">
        <h2 className="font-mono text-xs uppercase tracking-wider text-muted-foreground">
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
      <section className="flex h-full flex-col gap-3">
        <header className="flex items-center justify-between">
          <div className="flex items-baseline gap-3">
            <h1 className="font-mono text-lg font-semibold tracking-tight">Memory</h1>
            <span className="font-mono text-xs text-muted-foreground">
              {data ? `${data.rows.length} records` : '—'}
            </span>
          </div>
          <Button onClick={onCreateClick} size="sm" className="gap-2">
            <Plus className="h-4 w-4" />
            New memory
          </Button>
        </header>

        {isLoading && <ListSkeleton />}
        {error && (
          <div className="text-sm text-destructive">
            Failed to load memory: {(error as Error).message}
          </div>
        )}
        {data && data.rows.length === 0 && <EmptyState onCreateClick={onCreateClick} />}
        {data && data.rows.length > 0 && (
          <ul
            data-testid="memory-list"
            className="grid grid-cols-1 gap-2 overflow-y-auto md:grid-cols-2"
          >
            {data.rows.map((row) => (
              <li key={row.id}>
                <MemoryCard row={row} />
              </li>
            ))}
          </ul>
        )}
      </section>
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
        'block rounded-md border border-border/70 bg-background/80 p-3 text-left shadow-sm transition-all',
        'hover:-translate-y-0.5 hover:border-primary/40 hover:shadow-[0_0_0_1px_hsl(var(--primary)/0.25)]',
      )}
    >
      <div className="mb-1 flex items-center justify-between gap-2">
        <KindBadge kind={row.kind} />
        <span className="font-mono text-[0.65rem] uppercase tracking-wider text-muted-foreground">
          {row.id.slice(0, 14)}
        </span>
      </div>
      <div className="text-sm leading-snug text-foreground">{row.title}</div>
      <div className="mt-2 flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
        {row.trust && <span className="rounded bg-muted px-1.5 py-0.5">{row.trust}</span>}
        {row.riskClass && (
          <span className="rounded bg-muted px-1.5 py-0.5">{row.riskClass}</span>
        )}
        {row.stale && (
          <span className="inline-flex items-center gap-1 text-amber-400">
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
        'rounded-sm bg-primary/15 px-1.5 py-0.5 font-mono text-[0.625rem] font-semibold uppercase tracking-wider text-primary',
      )}
    >
      {kind}
    </span>
  )
}

function ListSkeleton() {
  return (
    <div className="grid grid-cols-1 gap-2 md:grid-cols-2">
      {Array.from({ length: 6 }).map((_, i) => (
        <Skeleton key={i} className="h-20 w-full" />
      ))}
    </div>
  )
}

function EmptyState({ onCreateClick }: { onCreateClick: () => void }) {
  return (
    <div className="flex flex-1 items-center justify-center p-8">
      <div className="max-w-md rounded-xl border border-border/70 bg-card/50 p-8 text-center shadow-[0_0_0_1px_hsl(var(--border)/0.4)_inset]">
        <h2 className="font-mono text-2xl font-semibold">No memory yet</h2>
        <p className="mt-2 text-sm text-muted-foreground">
          Click the button below to capture the first one. Incidents, findings,
          runbooks, decisions, gotchas and freeform notes all live here.
        </p>
        <Button onClick={onCreateClick} className="mt-6 gap-2">
          <Plus className="h-4 w-4 text-primary-foreground" />
          Create memory
        </Button>
      </div>
    </div>
  )
}

/**
 * Memory search UI.
 *
 * - Top: query input + mode segmented control + filter row.
 * - Below: ranked hits, plus a non-fatal yellow-amber warnings banner for
 *   ops-layer fallbacks (e.g. "embedder unavailable; falling back to lexical").
 * - URL search params drive state so links stay shareable (`?q=&mode=&kind=`).
 * - `?similarTo=<id>` deep-link short-circuits into `/api/memory/similar/:id`.
 * - Debounce: typing pauses 300ms before firing the query.
 */
import { useEffect, useMemo, useState, type ReactNode } from 'react'
import { Link, useNavigate, useSearch } from '@tanstack/react-router'
import { AlertTriangle, Search as SearchIcon } from 'lucide-react'
import type { MemoryKind } from '@/api/types/MemoryKind'
import type { SearchMode } from '@/api/types/SearchMode'
import type { TrustStateInput } from '@/api/types/TrustStateInput'
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
import { useMemorySearch, useMemorySimilar } from './use-memory-query'
import { resultTarget, type ResultTarget } from '@/features/search/result-nav'
import { KindBadge } from './memory-list'
import { TrustBadge } from '@/features/trust/trust-badge'
import { ModeSegmented } from '@/features/search/mode-segmented'
import { MEMORY_KINDS, TRUST_STATES } from './types'

export interface SearchRouteParams {
  q?: string
  mode?: SearchMode
  kind?: MemoryKind
  trust?: TrustStateInput
  scope?: string
  includeQuarantine?: boolean
  similarTo?: string
}

export function MemorySearch() {
  const search = useSearch({ from: '/memory/search' }) as SearchRouteParams
  const navigate = useNavigate({ from: '/memory/search' })

  // Local query mirror — debounced into the URL so the cache key stabilises.
  const [draftQuery, setDraftQuery] = useState(search.q ?? '')
  useEffect(() => {
    // If the URL changes externally (e.g. similarTo link), resync.
    setDraftQuery(search.q ?? '')
  }, [search.q])

  useEffect(() => {
    if (search.similarTo) return // similarTo branch ignores `q`
    if (draftQuery === (search.q ?? '')) return
    const handle = setTimeout(() => {
      navigate({
        to: '/memory/search',
        search: (prev: SearchRouteParams) => ({ ...prev, q: draftQuery || undefined }),
      })
    }, 300)
    return () => clearTimeout(handle)
  }, [draftQuery, search.q, search.similarTo, navigate])

  const mode = search.mode ?? 'auto'

  const searchParams = useMemo(
    () => ({
      q: search.q ?? '',
      mode,
      kind: search.kind ?? null,
      trust: search.trust ?? null,
      scope: search.scope ?? null,
      includeQuarantine: !!search.includeQuarantine,
    }),
    [search.q, mode, search.kind, search.trust, search.scope, search.includeQuarantine],
  )

  const lexicalQuery = useMemorySearch(searchParams, !search.similarTo)
  const similarQuery = useMemorySimilar(search.similarTo ?? undefined, 10)

  const active = search.similarTo ? similarQuery : lexicalQuery
  const data = active.data
  const isLoading = active.isLoading
  const error = active.error

  function updateSearch(next: Partial<SearchRouteParams>) {
    navigate({
      to: '/memory/search',
      search: (prev: SearchRouteParams) => {
        const merged: SearchRouteParams = { ...prev, ...next }
        const cleaned: SearchRouteParams = {}
        for (const [k, v] of Object.entries(merged)) {
          if (v === undefined || v === '' || v === false) continue
          ;(cleaned as Record<string, unknown>)[k] = v
        }
        return cleaned
      },
    })
  }

  function clearSimilar() {
    updateSearch({ similarTo: undefined })
  }

  return (
    <div className="mx-auto max-w-4xl space-y-4 px-6 py-5">
      <PageHeader
        title="Search memory"
        subtitle={
          search.similarTo ? (
            <div className="flex items-center justify-between gap-3 rounded-lg border border-primary/40 bg-primary/5 px-3 py-2 text-sm">
              <span>
                Similar to{' '}
                <Link
                  to="/memory/$id"
                  params={{ id: search.similarTo }}
                  className="font-mono text-primary hover:underline"
                >
                  {search.similarTo.slice(0, 14)}
                </Link>
              </span>
              <Button size="sm" variant="ghost" onClick={clearSimilar}>
                Clear
              </Button>
            </div>
          ) : (
            <div className="flex items-center gap-2 rounded-lg border border-input bg-card px-3">
              <SearchIcon className="h-4 w-4 shrink-0 text-muted-foreground" />
              <Input
                autoFocus
                value={draftQuery}
                onChange={(e) => setDraftQuery(e.target.value)}
                placeholder="What are you looking for?"
                className="flex-1 border-0 bg-transparent px-0 shadow-none focus-visible:ring-0"
                data-shortcut-target="search"
              />
            </div>
          )
        }
      />

      {/* Mode + filters (only when in regular search mode). */}
      {!search.similarTo && (
        <div className="grid grid-cols-1 gap-3 md:grid-cols-[1fr_auto] md:items-end">
          <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
            <div className="space-y-1">
              <Label className="text-xs">Kind</Label>
              <Select
                value={search.kind ?? '__all__'}
                onValueChange={(v) =>
                  updateSearch({ kind: v === '__all__' ? undefined : (v as MemoryKind) })
                }
              >
                <SelectTrigger className="h-8 text-xs">
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
            <div className="space-y-1">
              <Label className="text-xs">Trust</Label>
              <Select
                value={search.trust ?? '__any__'}
                onValueChange={(v) =>
                  updateSearch({
                    trust: v === '__any__' ? undefined : (v as TrustStateInput),
                  })
                }
              >
                <SelectTrigger className="h-8 text-xs">
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
            <div className="space-y-1">
              <Label className="text-xs">Scope</Label>
              <Input
                className="h-8 text-xs"
                value={search.scope ?? ''}
                onChange={(e) => updateSearch({ scope: e.target.value || undefined })}
                placeholder="scope id"
              />
            </div>
            <label className="flex items-end gap-2 pb-1 text-xs">
              <Input
                type="checkbox"
                className="h-4 w-4 accent-primary"
                checked={!!search.includeQuarantine}
                onChange={(e) =>
                  updateSearch({ includeQuarantine: e.target.checked || undefined })
                }
              />
              <span>Include quarantine</span>
            </label>
          </div>
          <ModeSegmented
            value={mode}
            onChange={(m) => updateSearch({ mode: m === 'auto' ? undefined : m })}
          />
        </div>
      )}

      {/* Warnings banner */}
      {data?.warnings && data.warnings.length > 0 && (
        <div
          role="alert"
          data-testid="search-warnings"
          className="flex items-start gap-2 rounded-lg border border-warning/40 bg-warning/10 px-3 py-2 text-sm text-warning"
        >
          <AlertTriangle className="mt-0.5 h-4 w-4 flex-shrink-0" />
          <div className="space-y-1">
            {data.warnings.map((w, i) => (
              <p key={i}>{w}</p>
            ))}
            {(mode === 'vector' || mode === 'hybrid') && (
              <Button
                size="sm"
                variant="outline"
                className="mt-1 h-7"
                onClick={() => updateSearch({ mode: 'lexical' })}
              >
                Try keyword instead
              </Button>
            )}
          </div>
        </div>
      )}

      {/* Results */}
      {isLoading && <ResultsSkeleton />}
      {error && (
        <div className="text-sm text-destructive">
          Search failed: {(error as Error).message}
        </div>
      )}
      {data && data.hits.length === 0 && (
        <p className="rounded-lg border border-dashed border-border px-3 py-6 text-center text-sm text-muted-foreground">
          No matches.
        </p>
      )}
      {data && data.hits.length > 0 && (
        <ul data-testid="search-results" className="space-y-2.5">
          {data.hits.map((hit) => (
            <li key={hit.id}>
              <HitLink
                target={resultTarget(hit.kind, hit.id)}
                className={cn(
                  'block rounded-lg border border-border bg-card p-3 shadow-elevation-1 transition-colors',
                  'hover:border-primary/40 hover:bg-surface-2',
                )}
              >
                <div className="mb-1.5 flex items-center justify-between gap-2">
                  <div className="flex flex-wrap items-center gap-2">
                    <KindBadge kind={hit.kind} />
                    <span className="font-mono text-[0.65rem] uppercase tracking-wider text-muted-foreground">
                      {hit.id.slice(0, 14)}
                    </span>
                    {hit.quarantine && (
                      <span className="rounded-full bg-warning/15 px-2 py-0.5 font-mono text-[0.625rem] uppercase tracking-wider text-warning">
                        quarantined
                      </span>
                    )}
                  </div>
                  <div className="flex items-center gap-2 font-mono text-[0.65rem] text-muted-foreground">
                    <span className="rounded-full bg-muted px-2 py-0.5 uppercase tracking-wider">
                      {hit.mode}
                    </span>
                    <span className="tabular-nums">{hit.score.toFixed(3)}</span>
                  </div>
                </div>
                <div className="text-sm font-medium leading-snug">{hit.title}</div>
                <div className="mt-2">
                  <TrustBadge state={hit.trust} />
                </div>
              </HitLink>
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}

/**
 * Render a hit card as a link to its kind-appropriate detail route. Falls back
 * to a non-navigable card when the hit cannot be linked (e.g. a malformed
 * synthetic doc id) so a dead-end click can never 404.
 *
 * The per-route `switch` keeps TanStack Router's typed `to`/`params` happy —
 * a union `to` would erase the param type.
 */
function HitLink({
  target,
  className,
  children,
}: {
  target: ResultTarget | null
  className: string
  children: ReactNode
}) {
  if (!target) {
    return <div className={cn(className, 'cursor-default')}>{children}</div>
  }
  const params = target.params as { id: string }
  switch (target.to) {
    case '/tickets/$id':
      return (
        <Link to="/tickets/$id" params={params} className={className}>
          {children}
        </Link>
      )
    case '/scope/$id':
      return (
        <Link to="/scope/$id" params={params} className={className}>
          {children}
        </Link>
      )
    case '/identity/$id':
      return (
        <Link to="/identity/$id" params={params} className={className}>
          {children}
        </Link>
      )
    case '/memory/$id':
      return (
        <Link to="/memory/$id" params={params} className={className}>
          {children}
        </Link>
      )
    default:
      return <div className={cn(className, 'cursor-default')}>{children}</div>
  }
}

function ResultsSkeleton() {
  return (
    <div className="space-y-2">
      {Array.from({ length: 4 }).map((_, i) => (
        <Skeleton key={i} className="h-16 w-full" />
      ))}
    </div>
  )
}

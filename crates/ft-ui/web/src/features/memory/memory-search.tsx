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
import { useEffect, useMemo, useState } from 'react'
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
import { cn } from '@/lib/utils'
import { useMemorySearch, useMemorySimilar } from './use-memory-query'
import { KindBadge } from './memory-list'
import { MEMORY_KINDS, SEARCH_MODES, TRUST_STATES } from './types'

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
    <div className="mx-auto max-w-4xl space-y-4 p-6">
      <header className="space-y-2">
        <h1 className="font-mono text-lg font-semibold tracking-tight">Search memory</h1>
        {search.similarTo ? (
          <div className="flex items-center justify-between gap-3 rounded-md border border-primary/40 bg-primary/5 px-3 py-2 text-sm">
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
          <div className="flex items-center gap-2">
            <SearchIcon className="h-4 w-4 text-muted-foreground" />
            <Input
              autoFocus
              value={draftQuery}
              onChange={(e) => setDraftQuery(e.target.value)}
              placeholder="What are you looking for?"
              className="flex-1"
              data-shortcut-target="search"
            />
          </div>
        )}
      </header>

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
          className="flex items-start gap-2 rounded-md border border-amber-400/40 bg-amber-400/10 px-3 py-2 text-sm text-amber-300"
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
        <p className="rounded-md border border-dashed border-border/60 px-3 py-6 text-center text-sm text-muted-foreground">
          No matches.
        </p>
      )}
      {data && data.hits.length > 0 && (
        <ul data-testid="search-results" className="space-y-2">
          {data.hits.map((hit) => (
            <li key={hit.id}>
              <Link
                to="/memory/$id"
                params={{ id: hit.id }}
                className={cn(
                  'block rounded-md border border-border/70 bg-background/80 p-3 transition-all',
                  'hover:-translate-y-0.5 hover:border-primary/40',
                )}
              >
                <div className="mb-1 flex items-center justify-between gap-2">
                  <div className="flex items-center gap-2">
                    <KindBadge kind={hit.kind} />
                    <span className="font-mono text-[0.65rem] uppercase tracking-wider text-muted-foreground">
                      {hit.id.slice(0, 14)}
                    </span>
                    {hit.quarantine && (
                      <span className="rounded bg-amber-400/15 px-1.5 py-0.5 text-[0.625rem] text-amber-300">
                        quarantined
                      </span>
                    )}
                  </div>
                  <div className="flex items-center gap-2 font-mono text-[0.65rem] text-muted-foreground">
                    <span className="rounded bg-muted px-1.5 py-0.5">{hit.mode}</span>
                    <span>{hit.score.toFixed(3)}</span>
                  </div>
                </div>
                <div className="text-sm">{hit.title}</div>
                <div className="mt-1 text-xs text-muted-foreground">
                  trust: {hit.trust}
                </div>
              </Link>
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}

function ModeSegmented({
  value,
  onChange,
}: {
  value: SearchMode
  onChange: (m: SearchMode) => void
}) {
  return (
    <div
      role="radiogroup"
      aria-label="Search mode"
      className="inline-flex rounded-md border border-border/60 bg-card/60 p-1"
    >
      {SEARCH_MODES.map((m) => (
        <button
          key={m}
          type="button"
          role="radio"
          aria-checked={m === value}
          onClick={() => onChange(m)}
          className={cn(
            'rounded px-3 py-1 text-xs font-mono uppercase tracking-wider transition-colors',
            m === value
              ? 'bg-primary text-primary-foreground'
              : 'text-muted-foreground hover:text-foreground',
          )}
        >
          {m}
        </button>
      ))}
    </div>
  )
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

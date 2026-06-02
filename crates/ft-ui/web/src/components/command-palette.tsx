/**
 * Global command palette — opened with Cmd/Ctrl+K (wired in
 * `ShortcutsProvider`). Built on `cmdk`, which composes a Radix Dialog under
 * the hood, so focus-trapping and Esc-to-close come for free.
 *
 * Two surfaces, both always available:
 *   1. Static nav shortcuts (Board / Memory / Scope / Identity / Audit) and
 *      actions (create ticket).
 *   2. LIVE cross-domain search — typing a query (debounced) hits
 *      `GET /api/search` via `useGlobalSearch` and renders ranked hits across
 *      tasks / bugs / epics / memories / scope / identity / audit, each with a
 *      kind badge + trust badge. Selecting a hit navigates to its record.
 *
 * A row of kind filter chips narrows the live results. cmdk's built-in
 * substring filtering is disabled (`shouldFilter={false}`) for the results
 * group so the server's ranking is authoritative; the nav group is matched
 * manually against the query instead.
 *
 * Styling follows §5/§6 of the redesign spec: surface-3 with elevation-2.
 */
import * as React from 'react'
import { Command } from 'cmdk'
import * as DialogPrimitive from '@radix-ui/react-dialog'
import { useNavigate } from '@tanstack/react-router'
import {
  KanbanSquare,
  Brain,
  Boxes,
  Users,
  ScrollText,
  Plus,
  Search,
  Loader2,
} from 'lucide-react'
import type { SearchKind } from '@/api/types/SearchKind'
import type { SearchMode } from '@/api/types/SearchMode'
import { Badge } from '@/components/ui/badge'
import { TrustBadge } from '@/features/trust/trust-badge'
import {
  SEARCH_KIND_CHIPS,
  kindBadgeVariant,
  resultTarget,
} from '@/features/search/result-nav'
import { ModeSegmented } from '@/features/search/mode-segmented'
import { useGlobalSearch } from '@/features/search/use-global-search'
import { cn } from '@/lib/utils'

interface CommandPaletteProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

export function CommandPalette({ open, onOpenChange }: CommandPaletteProps) {
  const navigate = useNavigate()

  const [query, setQuery] = React.useState('')
  const [debounced, setDebounced] = React.useState('')
  const [kinds, setKinds] = React.useState<SearchKind[]>([])
  const [mode, setMode] = React.useState<SearchMode>('auto')

  // Reset transient state when the palette closes so a stale query never
  // lingers behind the next invocation. Done in the change handler (rather than
  // an effect) to avoid a synchronous setState-in-effect cascade.
  const handleOpenChange = React.useCallback(
    (next: boolean) => {
      if (!next) {
        setQuery('')
        setDebounced('')
        setKinds([])
        setMode('auto')
      }
      onOpenChange(next)
    },
    [onOpenChange],
  )

  // Debounce the query (250ms) into the value that drives the network request.
  React.useEffect(() => {
    const handle = setTimeout(() => setDebounced(query), 250)
    return () => clearTimeout(handle)
  }, [query])

  const searchResult = useGlobalSearch(
    // `auto` is the default and the backend's default, so omit it from the
    // request to keep the common-case URL clean; only send an explicit mode.
    { q: debounced, kinds, mode: mode === 'auto' ? undefined : mode, limit: 15 },
    open,
  )
  const hits = searchResult.data?.hits ?? []
  const searching = debounced.trim().length > 0

  const run = React.useCallback(
    (fn: () => void) => {
      handleOpenChange(false)
      // Defer the action until after the dialog has begun closing so focus
      // restoration doesn't fight the navigation.
      requestAnimationFrame(fn)
    },
    [handleOpenChange],
  )

  const toggleKind = React.useCallback((kind: SearchKind) => {
    setKinds((prev) =>
      prev.includes(kind) ? prev.filter((k) => k !== kind) : [...prev, kind],
    )
  }, [])

  function selectHit(kind: string, id: string) {
    const target = resultTarget(kind, id)
    if (!target) return
    run(() =>
      void navigate({
        // The router's typed `to`/`params` are validated at runtime against
        // the generated route tree; the cast keeps the dynamic mapping ergonomic.
        to: target.to as never,
        params: target.params as never,
      }),
    )
  }

  return (
    <DialogPrimitive.Root open={open} onOpenChange={handleOpenChange}>
      <DialogPrimitive.Portal>
        <DialogPrimitive.Overlay
          className={cn(
            'fixed inset-0 z-50 bg-black/70 backdrop-blur-sm',
            'data-[state=open]:animate-in data-[state=closed]:animate-out',
            'data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0',
          )}
        />
        <DialogPrimitive.Content
          aria-label="Command palette"
          className={cn(
            'fixed left-[50%] top-[20%] z-50 w-full max-w-lg translate-x-[-50%]',
            'overflow-hidden rounded-lg border border-border bg-surface-3 shadow-elevation-2',
            'data-[state=open]:animate-in data-[state=closed]:animate-out',
            'data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0',
            'data-[state=closed]:zoom-out-95 data-[state=open]:zoom-in-95',
          )}
        >
          <DialogPrimitive.Title className="sr-only">Command palette</DialogPrimitive.Title>
          <DialogPrimitive.Description className="sr-only">
            Search across tasks, memories, scopes, identities and audit, or jump to a section
          </DialogPrimitive.Description>
          <Command
            loop
            // Disable cmdk's substring filter — the search group is server
            // ranked and the nav group is matched manually below.
            shouldFilter={false}
            className="[&_[cmdk-group-heading]]:px-3 [&_[cmdk-group-heading]]:py-1.5 [&_[cmdk-group-heading]]:text-xs [&_[cmdk-group-heading]]:font-medium [&_[cmdk-group-heading]]:uppercase [&_[cmdk-group-heading]]:tracking-wide [&_[cmdk-group-heading]]:text-muted-foreground"
          >
            <div className="flex items-center gap-2 border-b border-border px-3">
              {searching && searchResult.isFetching ? (
                <Loader2 className="h-4 w-4 shrink-0 animate-spin text-muted-foreground" />
              ) : (
                <Search className="h-4 w-4 shrink-0 text-muted-foreground" />
              )}
              <Command.Input
                autoFocus
                value={query}
                onValueChange={setQuery}
                placeholder="Search tasks, memories, scopes…"
                className="flex h-11 w-full bg-transparent py-3 text-sm outline-none placeholder:text-muted-foreground disabled:cursor-not-allowed disabled:opacity-50"
              />
            </div>

            {/* Kind filter chips. */}
            <div className="flex flex-wrap gap-1.5 border-b border-border px-3 py-2">
              {SEARCH_KIND_CHIPS.map((kind) => {
                const active = kinds.includes(kind)
                return (
                  <button
                    key={kind}
                    type="button"
                    aria-pressed={active}
                    data-testid={`kind-chip-${kind}`}
                    onClick={() => toggleKind(kind)}
                    className={cn(
                      'rounded-full px-2 py-0.5 font-mono text-[0.625rem] font-semibold uppercase tracking-wider transition-colors',
                      active
                        ? 'bg-primary text-primary-foreground'
                        : 'bg-muted text-muted-foreground hover:text-foreground',
                    )}
                  >
                    {kind}
                  </button>
                )
              })}
            </div>

            {/* Search-mode selector. `auto` runs hybrid when embeddings are
                available; the rest force a single strategy. */}
            <div className="flex items-center gap-2 border-b border-border px-3 py-2">
              <span className="font-mono text-[0.625rem] uppercase tracking-wider text-muted-foreground">
                Mode
              </span>
              <ModeSegmented value={mode} onChange={setMode} dense />
            </div>

            <Command.List className="max-h-80 overflow-y-auto overflow-x-hidden p-1.5">
              {/* Live cross-domain results. */}
              {searching && (
                <Command.Group heading="Results">
                  {hits.length === 0 && !searchResult.isFetching && (
                    <p className="px-3 py-4 text-center text-sm text-muted-foreground">
                      No matches.
                    </p>
                  )}
                  {searchResult.isError && (
                    <p className="px-3 py-4 text-center text-sm text-destructive">
                      Search failed: {(searchResult.error as Error).message}
                    </p>
                  )}
                  <ul data-testid="palette-results">
                    {hits.map((hit) => (
                      <Command.Item
                        key={hit.id}
                        value={hit.id}
                        asChild
                        onSelect={() => selectHit(hit.kind, hit.id)}
                      >
                        <li
                          className={cn(
                            'flex cursor-pointer select-none flex-col gap-1 rounded-md px-3 py-2 text-sm outline-none',
                            'text-foreground transition-colors',
                            'data-[selected=true]:bg-primary/10',
                          )}
                        >
                          <div className="flex items-center gap-2">
                            <Badge variant={kindBadgeVariant(hit.kind)} className="shrink-0">
                              {hit.kind}
                            </Badge>
                            <span className="truncate font-medium leading-snug">
                              {hit.title || hit.id}
                            </span>
                          </div>
                          <div className="flex items-center gap-2 text-[0.65rem] text-muted-foreground">
                            <TrustBadge state={hit.trust} hideIcon />
                            {hit.scope && (
                              <span className="truncate font-mono">{hit.scope}</span>
                            )}
                            <span className="ml-auto font-mono tabular-nums">
                              {hit.score.toFixed(2)}
                            </span>
                          </div>
                        </li>
                      </Command.Item>
                    ))}
                  </ul>
                </Command.Group>
              )}

              {/* Static navigation — always available; filtered by query. */}
              <NavGroup query={query} run={run} navigate={navigate} />
            </Command.List>
          </Command>
        </DialogPrimitive.Content>
      </DialogPrimitive.Portal>
    </DialogPrimitive.Root>
  )
}

/** Substring match used to filter the static nav/action items by query. */
function matches(query: string, label: string, keywords: string[]): boolean {
  const q = query.trim().toLowerCase()
  if (!q) return true
  if (label.toLowerCase().includes(q)) return true
  return keywords.some((k) => k.toLowerCase().includes(q))
}

function NavGroup({
  query,
  run,
  navigate,
}: {
  query: string
  run: (fn: () => void) => void
  navigate: ReturnType<typeof useNavigate>
}) {
  const navItems: Array<{
    icon: React.ReactNode
    label: string
    keywords: string[]
    onSelect: () => void
  }> = [
    {
      icon: <KanbanSquare className="h-4 w-4" />,
      label: 'Board',
      keywords: ['tickets', 'kanban', 'home'],
      onSelect: () => run(() => void navigate({ to: '/' })),
    },
    {
      icon: <Brain className="h-4 w-4" />,
      label: 'Memory',
      keywords: ['notes', 'recall'],
      onSelect: () => run(() => void navigate({ to: '/memory' })),
    },
    {
      icon: <Boxes className="h-4 w-4" />,
      label: 'Scope',
      keywords: ['boundary', 'boundaries'],
      onSelect: () => run(() => void navigate({ to: '/scope' })),
    },
    {
      icon: <Users className="h-4 w-4" />,
      label: 'Identity',
      keywords: ['actors', 'people'],
      onSelect: () => run(() => void navigate({ to: '/identity' })),
    },
    {
      icon: <ScrollText className="h-4 w-4" />,
      label: 'Audit',
      keywords: ['lineage', 'diff', 'history'],
      onSelect: () => run(() => void navigate({ to: '/audit' })),
    },
  ]

  const actionItems: Array<{
    icon: React.ReactNode
    label: string
    keywords: string[]
    onSelect: () => void
  }> = [
    {
      icon: <Plus className="h-4 w-4" />,
      label: 'Create ticket',
      keywords: ['new', 'add', 'issue'],
      onSelect: () =>
        run(() => void navigate({ to: '/', search: { create: true } as never })),
    },
  ]

  const visibleNav = navItems.filter((i) => matches(query, i.label, i.keywords))
  const visibleActions = actionItems.filter((i) => matches(query, i.label, i.keywords))

  return (
    <>
      {visibleNav.length > 0 && (
        <Command.Group heading="Navigate">
          {visibleNav.map((i) => (
            <PaletteItem key={i.label} icon={i.icon} label={i.label} onSelect={i.onSelect} />
          ))}
        </Command.Group>
      )}
      {visibleActions.length > 0 && (
        <Command.Group heading="Actions">
          {visibleActions.map((i) => (
            <PaletteItem key={i.label} icon={i.icon} label={i.label} onSelect={i.onSelect} />
          ))}
        </Command.Group>
      )}
    </>
  )
}

function PaletteItem({
  icon,
  label,
  onSelect,
}: {
  icon: React.ReactNode
  label: string
  onSelect: () => void
}) {
  return (
    <Command.Item
      value={label}
      onSelect={onSelect}
      className={cn(
        'flex cursor-pointer select-none items-center gap-2.5 rounded-md px-3 py-2 text-sm outline-none',
        'text-foreground transition-colors',
        'data-[selected=true]:bg-primary/10 data-[selected=true]:text-primary',
        'aria-disabled:pointer-events-none aria-disabled:opacity-50',
      )}
    >
      <span className="text-muted-foreground [[data-selected=true]_&]:text-primary">{icon}</span>
      <span>{label}</span>
    </Command.Item>
  )
}

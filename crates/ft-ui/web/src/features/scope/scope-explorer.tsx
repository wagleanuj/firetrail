/**
 * Read-only scope explorer.
 *
 * Layout: filterable list of scopes on the left, detail panel on the right.
 * The path-to-owners resolver lives at the top as a single-line tool —
 * resolving a path is a quick query, not a workflow that deserves a modal.
 *
 * V1 deliberately exposes no editing. The CODEOWNERS file is the source of
 * truth; users who want to change it edit the file directly and the next
 * load picks it up.
 */
import { useState, useMemo } from 'react'
import { Link, useNavigate } from '@tanstack/react-router'
import { Loader2, Search, FileCode2, Users, ChevronRight, FolderTree } from 'lucide-react'
import { AnimatePresence, motion, useReducedMotion } from 'framer-motion'
import { LIST_STAGGER, ROUTE_TRANSITION, reducedTransition } from '@/lib/motion'
import { EmptyState } from '@/components/ui/empty-state'
import type { ScopeSummary } from '@/api/types/ScopeSummary'
import { Input } from '@/components/ui/input'
import { Button } from '@/components/ui/button'
import { Skeleton } from '@/components/ui/skeleton'
import { Badge } from '@/components/ui/badge'
import { Card } from '@/components/ui/card'
import { PageHeader } from '@/components/page-header'
import { Tabs, TabsList, TabsTrigger, TabsContent } from '@/components/ui/tabs'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { cn } from '@/lib/utils'
import {
  useScopeList,
  useScopeAliases,
  useScopeShow,
  useResolveOwners,
} from './use-scope-query'

interface ScopeExplorerProps {
  selectedId?: string
}

export function ScopeExplorer({ selectedId }: ScopeExplorerProps) {
  const navigate = useNavigate()
  const list = useScopeList()
  const [filter, setFilter] = useState('')
  const reduced = useReducedMotion() ?? false
  const transition = reducedTransition(reduced, ROUTE_TRANSITION)

  const filtered = useMemo(() => {
    if (!list.data) return []
    const q = filter.trim().toLowerCase()
    if (!q) return list.data.scopes
    return list.data.scopes.filter(
      (s) =>
        s.id.toLowerCase().includes(q) ||
        s.name.toLowerCase().includes(q) ||
        s.aliases.some((a) => a.toLowerCase().includes(q)) ||
        s.appliesTo.some((g) => g.toLowerCase().includes(q)),
    )
  }, [list.data, filter])

  return (
    <div className="mx-auto flex h-full max-w-6xl flex-col gap-6 px-6 py-6">
      <Tabs defaultValue="scopes" className="flex flex-1 flex-col gap-6">
        <PageHeader
          title="Scope"
          subtitle="Read-only view of scopes, CODEOWNERS rules, and alias bindings."
          tabs={
            <TabsList>
              <TabsTrigger value="scopes">Scopes</TabsTrigger>
              <TabsTrigger value="aliases">Aliases</TabsTrigger>
            </TabsList>
          }
        />

        <OwnersResolver />

        <TabsContent value="scopes" className="mt-0">
          <div className="grid grid-cols-1 gap-6 lg:grid-cols-[20rem_1fr]">
            <aside className="flex flex-col gap-2.5">
              <div className="relative">
                <Search className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted-foreground" />
                <Input
                  value={filter}
                  onChange={(e) => setFilter(e.target.value)}
                  placeholder="Filter scopes…"
                  className="pl-8"
                  data-shortcut-target="search"
                />
              </div>
              {list.isLoading && (
                <div className="space-y-2.5">
                  {Array.from({ length: 6 }).map((_, i) => (
                    <Skeleton key={i} className="h-14 w-full rounded-lg" />
                  ))}
                </div>
              )}
              {list.error && (
                <p className="text-sm text-destructive">
                  Failed to load scopes: {(list.error as Error).message}
                </p>
              )}
              <ul
                data-testid="scope-list"
                className="flex max-h-[62vh] flex-col gap-2.5 overflow-y-auto pr-0.5"
              >
                <AnimatePresence initial={!reduced}>
                {filtered.map((s, i) => (
                  <motion.li
                    key={s.id}
                    layout={!reduced}
                    initial={reduced ? false : { opacity: 0, y: 4 }}
                    animate={{ opacity: 1, y: 0 }}
                    exit={reduced ? { opacity: 0 } : { opacity: 0, y: -4 }}
                    transition={{ ...transition, delay: reduced ? 0 : Math.min(i, 12) * LIST_STAGGER }}
                  >
                    <button
                      type="button"
                      onClick={() =>
                        navigate({ to: '/scope/$id', params: { id: s.id } })
                      }
                      className={cn(
                        'group w-full rounded-lg border border-border bg-card p-3 text-left text-card-foreground shadow-elevation-1 transition-colors',
                        'hover:bg-surface-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring',
                        selectedId === s.id &&
                          'border-primary/40 bg-surface-2 ring-1 ring-primary/25 shadow-glow',
                      )}
                    >
                      <div className="flex items-center gap-2">
                        <FileCode2 className="h-3.5 w-3.5 shrink-0 text-primary" />
                        <span className="font-mono text-xs font-semibold">
                          {s.id}
                        </span>
                        {s.hasCodeowners && (
                          <Badge
                            variant="feature"
                            className="ml-auto rounded px-1.5 py-0 font-mono text-[0.6rem] uppercase tracking-wider"
                          >
                            owners
                          </Badge>
                        )}
                      </div>
                      <div className="mt-1 truncate text-xs text-muted-foreground">
                        {s.name}
                      </div>
                    </button>
                  </motion.li>
                ))}
                </AnimatePresence>
                {!list.isLoading && filtered.length === 0 && (
                  <li>
                    <EmptyState
                      icon={FolderTree}
                      title={filter ? 'No scopes match' : 'No scopes configured'}
                      description={
                        filter
                          ? 'Try a different filter or clear it to see every scope.'
                          : 'Scopes are defined in .firetrail/scopes.yaml. Add one and reload.'
                      }
                    />
                  </li>
                )}
              </ul>
            </aside>

            <section>
              {selectedId ? (
                <ScopeDetailPanel id={selectedId} />
              ) : (
                <EmptyDetail scopes={list.data?.scopes ?? []} />
              )}
            </section>
          </div>
        </TabsContent>

        <TabsContent value="aliases" className="mt-0">
          <AliasesPanel />
        </TabsContent>
      </Tabs>
    </div>
  )
}

function EmptyDetail({ scopes }: { scopes: ScopeSummary[] }) {
  if (scopes.length === 0) return null
  return (
    <div className="rounded-lg border border-dashed border-border px-6 py-12 text-center text-sm text-muted-foreground">
      <ChevronRight className="mx-auto mb-2 h-5 w-5" />
      Select a scope from the list to see its CODEOWNERS rules.
    </div>
  )
}

function ScopeDetailPanel({ id }: { id: string }) {
  const { data, isLoading, error } = useScopeShow(id)
  if (isLoading) {
    return (
      <div className="space-y-4">
        <div className="space-y-2">
          <Skeleton className="h-4 w-24" />
          <Skeleton className="h-7 w-1/3" />
        </div>
        <Skeleton className="h-28 w-full rounded-lg" />
        <Skeleton className="h-40 w-full rounded-lg" />
      </div>
    )
  }
  if (error) {
    return (
      <p className="text-sm text-destructive">
        Failed to load scope: {(error as Error).message}
      </p>
    )
  }
  if (!data) return null
  const { summary, codeowners } = data.scope

  return (
    <div className="space-y-5">
      <header className="space-y-1.5">
        <div className="flex items-center gap-1.5 font-mono text-xs uppercase tracking-wider text-muted-foreground">
          <Link to="/scope" className="transition-colors hover:text-primary">
            scope
          </Link>
          <span className="text-border">/</span>
          <span className="text-foreground">{summary.id}</span>
        </div>
        <h2 className="font-display text-xl font-semibold leading-snug tracking-tight">
          {summary.name}
        </h2>
      </header>

      <Card className="hover:bg-card">
        <dl className="grid grid-cols-1 gap-4 p-4 text-sm sm:grid-cols-2">
          <div>
            <dt className="font-mono text-[0.625rem] uppercase tracking-wider text-muted-foreground">
              Applies to
            </dt>
            <dd className="mt-1.5 space-y-1">
              {summary.appliesTo.length === 0 ? (
                <span className="text-muted-foreground">—</span>
              ) : (
                summary.appliesTo.map((g) => (
                  <code
                    key={g}
                    className="block break-all rounded bg-muted px-1.5 py-0.5 font-mono text-xs"
                  >
                    {g}
                  </code>
                ))
              )}
            </dd>
          </div>
          <div>
            <dt className="font-mono text-[0.625rem] uppercase tracking-wider text-muted-foreground">
              Aliases
            </dt>
            <dd className="mt-1.5 flex flex-wrap gap-1.5">
              {summary.aliases.length === 0 ? (
                <span className="text-muted-foreground">—</span>
              ) : (
                summary.aliases.map((a) => (
                  <span
                    key={a}
                    className="rounded bg-muted px-1.5 py-0.5 font-mono text-xs"
                  >
                    {a}
                  </span>
                ))
              )}
            </dd>
          </div>
        </dl>
      </Card>

      <div className="space-y-2.5">
        <div className="flex items-center gap-2">
          <Users className="h-4 w-4 text-primary" />
          <h3 className="text-sm font-medium uppercase tracking-wide text-muted-foreground">
            Code owners
          </h3>
        </div>
        {codeowners.length === 0 ? (
          <p className="rounded-lg border border-dashed border-border px-3 py-4 text-sm text-muted-foreground">
            No CODEOWNERS entries for this scope.
          </p>
        ) : (
          <Table data-testid="codeowners-table">
            <TableHeader>
              <TableRow>
                <TableHead>Pattern</TableHead>
                <TableHead>Owners</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {codeowners.map((row, i) => (
                <TableRow key={`${row.pattern}-${i}`}>
                  <TableCell>
                    <code className="font-mono text-xs">{row.pattern}</code>
                  </TableCell>
                  <TableCell>
                    <span className="font-mono text-xs">
                      {row.owners.join(', ')}
                    </span>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        )}
      </div>
    </div>
  )
}

function AliasesPanel() {
  const { data, isLoading, error } = useScopeAliases()
  if (isLoading) return <Skeleton className="h-40 w-full rounded-lg" />
  if (error) {
    return (
      <p className="text-sm text-destructive">
        Failed to load aliases: {(error as Error).message}
      </p>
    )
  }
  if (!data) return null
  if (data.aliases.length === 0) {
    return (
      <p className="rounded-lg border border-dashed border-border px-3 py-6 text-center text-sm text-muted-foreground">
        No aliases configured.
      </p>
    )
  }
  return (
    <Table>
      <TableHeader>
        <TableRow>
          <TableHead>Alias</TableHead>
          <TableHead>Scope id</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {data.aliases.map((a) => (
          <TableRow key={`${a.alias}-${a.scopeId}`}>
            <TableCell>
              <code className="font-mono text-xs">{a.alias}</code>
            </TableCell>
            <TableCell>
              <Link
                to="/scope/$id"
                params={{ id: a.scopeId }}
                className="font-mono text-xs text-primary hover:underline"
              >
                {a.scopeId}
              </Link>
            </TableCell>
          </TableRow>
        ))}
      </TableBody>
    </Table>
  )
}

function OwnersResolver() {
  const [path, setPath] = useState('')
  const resolve = useResolveOwners()
  return (
    <form
      className="flex items-center gap-2.5 rounded-lg border border-border bg-card p-3 shadow-elevation-1"
      onSubmit={(e) => {
        e.preventDefault()
        if (path.trim()) resolve.mutate(path.trim())
      }}
    >
      <span className="shrink-0 font-mono text-xs uppercase tracking-wider text-muted-foreground">
        Resolve path →
      </span>
      <Input
        value={path}
        onChange={(e) => setPath(e.target.value)}
        placeholder="e.g. crates/ft-core/src/lib.rs"
        className="flex-1"
      />
      <Button
        type="submit"
        size="sm"
        disabled={!path.trim() || resolve.isPending}
        className="gap-2"
      >
        {resolve.isPending && <Loader2 className="h-3 w-3 animate-spin" />}
        Resolve
      </Button>
      {resolve.data && (
        <span
          data-testid="owners-result"
          className="ml-2 truncate font-mono text-xs"
        >
          owners:{' '}
          <span className="text-primary">
            {resolve.data.owners.length === 0
              ? '— none —'
              : resolve.data.owners.join(', ')}
          </span>
        </span>
      )}
    </form>
  )
}

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
import { Loader2, Search, FileCode2, Users, ChevronRight } from 'lucide-react'
import type { ScopeSummary } from '@/api/types/ScopeSummary'
import { Input } from '@/components/ui/input'
import { Button } from '@/components/ui/button'
import { Skeleton } from '@/components/ui/skeleton'
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
    <div className="mx-auto flex h-full max-w-6xl flex-col gap-4 p-6">
      <header className="space-y-1">
        <h1 className="font-mono text-lg font-semibold tracking-tight">Scope</h1>
        <p className="text-sm text-muted-foreground">
          Read-only view of scopes, CODEOWNERS rules, and alias bindings.
        </p>
      </header>

      <OwnersResolver />

      <Tabs defaultValue="scopes" className="flex-1">
        <TabsList>
          <TabsTrigger value="scopes">Scopes</TabsTrigger>
          <TabsTrigger value="aliases">Aliases</TabsTrigger>
        </TabsList>

        <TabsContent value="scopes" className="mt-3">
          <div className="grid grid-cols-1 gap-4 lg:grid-cols-[18rem_1fr]">
            <aside className="flex flex-col gap-2">
              <div className="relative">
                <Search className="pointer-events-none absolute left-2 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted-foreground" />
                <Input
                  value={filter}
                  onChange={(e) => setFilter(e.target.value)}
                  placeholder="Filter scopes…"
                  className="pl-7"
                />
              </div>
              {list.isLoading && (
                <div className="space-y-1.5">
                  {Array.from({ length: 6 }).map((_, i) => (
                    <Skeleton key={i} className="h-12 w-full" />
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
                className="flex max-h-[60vh] flex-col gap-1 overflow-y-auto"
              >
                {filtered.map((s) => (
                  <li key={s.id}>
                    <button
                      type="button"
                      onClick={() =>
                        navigate({ to: '/scope/$id', params: { id: s.id } })
                      }
                      className={cn(
                        'group w-full rounded-md border border-border/70 bg-background/80 px-3 py-2 text-left transition-all',
                        'hover:-translate-y-0.5 hover:border-primary/40',
                        selectedId === s.id &&
                          'border-primary/60 bg-primary/5 shadow-[0_0_0_1px_hsl(var(--primary)/0.25)]',
                      )}
                    >
                      <div className="flex items-center gap-2">
                        <FileCode2 className="h-3.5 w-3.5 text-primary" />
                        <span className="font-mono text-xs font-semibold">
                          {s.id}
                        </span>
                        {s.hasCodeowners && (
                          <span className="ml-auto rounded bg-primary/15 px-1.5 py-0.5 font-mono text-[0.6rem] uppercase tracking-wider text-primary">
                            owners
                          </span>
                        )}
                      </div>
                      <div className="mt-0.5 truncate text-xs text-muted-foreground">
                        {s.name}
                      </div>
                    </button>
                  </li>
                ))}
                {!list.isLoading && filtered.length === 0 && (
                  <li className="rounded-md border border-dashed border-border/60 px-3 py-6 text-center text-sm text-muted-foreground">
                    No scopes match.
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

        <TabsContent value="aliases" className="mt-3">
          <AliasesPanel />
        </TabsContent>
      </Tabs>
    </div>
  )
}

function EmptyDetail({ scopes }: { scopes: ScopeSummary[] }) {
  if (scopes.length === 0) return null
  return (
    <div className="rounded-md border border-dashed border-border/60 px-6 py-12 text-center text-sm text-muted-foreground">
      <ChevronRight className="mx-auto mb-2 h-5 w-5" />
      Select a scope from the list to see its CODEOWNERS rules.
    </div>
  )
}

function ScopeDetailPanel({ id }: { id: string }) {
  const { data, isLoading, error } = useScopeShow(id)
  if (isLoading) {
    return (
      <div className="space-y-3">
        <Skeleton className="h-6 w-1/3" />
        <Skeleton className="h-4 w-1/2" />
        <Skeleton className="h-40 w-full" />
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
    <div className="space-y-4">
      <header className="space-y-1">
        <div className="flex items-center gap-2 font-mono text-xs uppercase tracking-wider text-muted-foreground">
          <Link to="/scope" className="hover:text-primary">
            scope
          </Link>
          <span>/</span>
          <span>{summary.id}</span>
        </div>
        <h2 className="text-xl font-semibold">{summary.name}</h2>
      </header>

      <dl className="grid grid-cols-1 gap-3 rounded-md border border-border/70 bg-background/60 p-4 text-sm sm:grid-cols-2">
        <div>
          <dt className="font-mono text-[0.625rem] uppercase tracking-wider text-muted-foreground">
            Applies to
          </dt>
          <dd className="mt-1 space-y-0.5">
            {summary.appliesTo.length === 0 ? (
              <span className="text-muted-foreground">—</span>
            ) : (
              summary.appliesTo.map((g) => (
                <code
                  key={g}
                  className="block break-all rounded bg-muted/60 px-1.5 py-0.5 font-mono text-xs"
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
          <dd className="mt-1 flex flex-wrap gap-1">
            {summary.aliases.length === 0 ? (
              <span className="text-muted-foreground">—</span>
            ) : (
              summary.aliases.map((a) => (
                <span
                  key={a}
                  className="rounded bg-muted/60 px-1.5 py-0.5 font-mono text-xs"
                >
                  {a}
                </span>
              ))
            )}
          </dd>
        </div>
      </dl>

      <div className="space-y-2">
        <div className="flex items-center gap-2">
          <Users className="h-4 w-4 text-primary" />
          <h3 className="font-mono text-xs uppercase tracking-wider text-muted-foreground">
            Code owners
          </h3>
        </div>
        {codeowners.length === 0 ? (
          <p className="rounded-md border border-dashed border-border/60 px-3 py-4 text-sm text-muted-foreground">
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
  if (isLoading) return <Skeleton className="h-40 w-full" />
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
      <p className="rounded-md border border-dashed border-border/60 px-3 py-6 text-center text-sm text-muted-foreground">
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
      className="flex items-center gap-2 rounded-md border border-border/70 bg-background/60 p-3"
      onSubmit={(e) => {
        e.preventDefault()
        if (path.trim()) resolve.mutate(path.trim())
      }}
    >
      <span className="font-mono text-xs uppercase tracking-wider text-muted-foreground">
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

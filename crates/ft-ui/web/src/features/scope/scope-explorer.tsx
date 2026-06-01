/**
 * Scope explorer + authoring surface.
 *
 * Layout: filterable list of scopes on the left (each row carries inline
 * edit / delete / reorder controls), a detail panel on the right, and a live
 * match-count preview. The path-to-owners resolver lives at the top as a
 * single-line tool — resolving a path is a quick query, not a workflow.
 *
 * Authoring writes through `POST/PUT/DELETE /api/scope` (+ `/reorder`); the
 * `.firetrail/scopes.yaml` file is regenerated server-side. Reordering is
 * first-class because resolution is **last-declared-wins** (like CODEOWNERS),
 * so declaration order *is* precedence.
 *
 * Progressive disclosure: a repo with no scopes is treated as a single unit,
 * so the empty state explains that calmly and only offers an opt-in "Add a
 * scope" (plus a suggest-only "Scaffold from directories" helper) — it never
 * nags.
 */
import { useState, useMemo } from 'react'
import { Link, useNavigate } from '@tanstack/react-router'
import {
  Loader2,
  Search,
  FileCode2,
  Users,
  ChevronRight,
  FolderTree,
  Plus,
  Pencil,
  Trash2,
  ChevronUp,
  ChevronDown,
  X,
  AlertTriangle,
  ListTree,
} from 'lucide-react'
import { AnimatePresence, motion, useReducedMotion } from 'framer-motion'
import { LIST_STAGGER, ROUTE_TRANSITION, reducedTransition } from '@/lib/motion'
import { EmptyState } from '@/components/ui/empty-state'
import type { ScopeSummary } from '@/api/types/ScopeSummary'
import { Input } from '@/components/ui/input'
import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import { Skeleton } from '@/components/ui/skeleton'
import { Badge } from '@/components/ui/badge'
import { Card } from '@/components/ui/card'
import { FilePathCombobox } from '@/components/ui/autocomplete'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
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
import { useFiles } from '@/features/files/use-files-query'
import { cn } from '@/lib/utils'
import {
  useScopeList,
  useScopeAliases,
  useScopeShow,
  useResolveOwners,
  useScopePreview,
  useAddScope,
  useEditScope,
  useRemoveScope,
  useReorderScopes,
} from './use-scope-query'

interface ScopeExplorerProps {
  selectedId?: string
}

export function ScopeExplorer({ selectedId }: ScopeExplorerProps) {
  const navigate = useNavigate()
  const list = useScopeList()
  const preview = useScopePreview()
  const reorder = useReorderScopes()
  const [filter, setFilter] = useState('')
  // The create form and the edit form share one piece of state: `null` = closed,
  // `{ mode: 'create' }` = new, `{ mode: 'edit', scope }` = editing an existing.
  const [form, setForm] = useState<FormState>(null)
  const reduced = useReducedMotion() ?? false
  const transition = reducedTransition(reduced, ROUTE_TRANSITION)

  const scopes = useMemo(() => list.data?.scopes ?? [], [list.data])
  const matchById = useMemo(() => {
    const m = new Map<string, number>()
    for (const row of preview.data?.scopes ?? []) m.set(row.id, row.matchCount)
    return m
  }, [preview.data])

  const filtered = useMemo(() => {
    const q = filter.trim().toLowerCase()
    if (!q) return scopes
    return scopes.filter(
      (s) =>
        s.id.toLowerCase().includes(q) ||
        s.name.toLowerCase().includes(q) ||
        s.aliases.some((a) => a.toLowerCase().includes(q)) ||
        s.appliesTo.some((g) => g.toLowerCase().includes(q)),
    )
  }, [scopes, filter])

  // Reorder operates on the full, unfiltered declaration order so precedence is
  // never silently rewritten by an active filter.
  function move(id: string, dir: -1 | 1) {
    const ids = scopes.map((s) => s.id)
    const idx = ids.indexOf(id)
    const next = idx + dir
    if (idx < 0 || next < 0 || next >= ids.length) return
    ;[ids[idx], ids[next]] = [ids[next], ids[idx]]
    reorder.mutate(ids)
  }

  const isEmpty = !list.isLoading && !list.error && scopes.length === 0

  return (
    <div className="mx-auto flex h-full max-w-6xl flex-col gap-6 px-6 py-6">
      <Tabs defaultValue="scopes" className="flex flex-1 flex-col gap-6">
        <PageHeader
          title="Scope"
          subtitle="Define and order scopes, view CODEOWNERS rules, and inspect alias bindings."
          tabs={
            <TabsList>
              <TabsTrigger value="scopes">Scopes</TabsTrigger>
              <TabsTrigger value="aliases">Aliases</TabsTrigger>
            </TabsList>
          }
        />

        <OwnersResolver />

        <TabsContent value="scopes" className="mt-0">
          {isEmpty ? (
            <ScopeEmptyState onAddScope={() => setForm({ mode: 'create' })} />
          ) : (
            <div className="grid grid-cols-1 gap-6 lg:grid-cols-[22rem_1fr]">
              <aside className="flex flex-col gap-2.5">
                <div className="flex items-center gap-2">
                  <div className="relative flex-1">
                    <Search className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted-foreground" />
                    <Input
                      value={filter}
                      onChange={(e) => setFilter(e.target.value)}
                      placeholder="Filter scopes…"
                      className="pl-8"
                      data-shortcut-target="search"
                    />
                  </div>
                  <Button
                    type="button"
                    size="sm"
                    variant="outline"
                    className="shrink-0 gap-1.5"
                    data-testid="scope-create-open"
                    onClick={() => setForm({ mode: 'create' })}
                  >
                    <Plus className="h-3.5 w-3.5" />
                    New
                  </Button>
                </div>

                <p className="px-0.5 text-[0.7rem] leading-snug text-muted-foreground">
                  Order is precedence — a later scope wins where globs overlap
                  (last-declared-wins, like CODEOWNERS).
                </p>

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
                    {filtered.map((s, i) => {
                      const fullIdx = scopes.findIndex((x) => x.id === s.id)
                      return (
                        <motion.li
                          key={s.id}
                          layout={!reduced}
                          initial={reduced ? false : { opacity: 0, y: 4 }}
                          animate={{ opacity: 1, y: 0 }}
                          exit={reduced ? { opacity: 0 } : { opacity: 0, y: -4 }}
                          transition={{
                            ...transition,
                            delay: reduced ? 0 : Math.min(i, 12) * LIST_STAGGER,
                          }}
                        >
                          <ScopeRow
                            scope={s}
                            selected={selectedId === s.id}
                            matchCount={matchById.get(s.id)}
                            canMoveUp={fullIdx > 0}
                            canMoveDown={fullIdx >= 0 && fullIdx < scopes.length - 1}
                            reorderPending={reorder.isPending}
                            onOpen={() =>
                              navigate({ to: '/scope/$id', params: { id: s.id } })
                            }
                            onEdit={() => setForm({ mode: 'edit', scope: s })}
                            onMoveUp={() => move(s.id, -1)}
                            onMoveDown={() => move(s.id, 1)}
                          />
                        </motion.li>
                      )
                    })}
                  </AnimatePresence>
                  {!list.isLoading && filtered.length === 0 && (
                    <li>
                      <EmptyState
                        icon={FolderTree}
                        title="No scopes match"
                        description="Try a different filter or clear it to see every scope."
                      />
                    </li>
                  )}
                </ul>

                <PreviewWarnings warnings={preview.data?.warnings ?? []} />
              </aside>

              <section>
                {form ? (
                  <ScopeForm
                    key={form.mode === 'edit' ? form.scope.id : 'create'}
                    state={form}
                    onClose={() => setForm(null)}
                  />
                ) : selectedId ? (
                  <ScopeDetailPanel id={selectedId} />
                ) : (
                  <EmptyDetail scopes={scopes} />
                )}
              </section>
            </div>
          )}
        </TabsContent>

        <TabsContent value="aliases" className="mt-0">
          <AliasesPanel />
        </TabsContent>
      </Tabs>
    </div>
  )
}

/** Create vs. edit form state. `null` means no form is open. */
type FormState =
  | null
  | { mode: 'create' }
  | { mode: 'edit'; scope: ScopeSummary }

/**
 * A single scope row: clicking the body opens the detail; the trailing control
 * cluster offers reorder (up/down), edit, and a confirm-guarded delete. The
 * live match count renders inline so authors see the blast radius of each
 * scope's globs without leaving the list.
 */
function ScopeRow({
  scope: s,
  selected,
  matchCount,
  canMoveUp,
  canMoveDown,
  reorderPending,
  onOpen,
  onEdit,
  onMoveUp,
  onMoveDown,
}: {
  scope: ScopeSummary
  selected: boolean
  matchCount?: number
  canMoveUp: boolean
  canMoveDown: boolean
  reorderPending: boolean
  onOpen: () => void
  onEdit: () => void
  onMoveUp: () => void
  onMoveDown: () => void
}) {
  return (
    <div
      className={cn(
        'group rounded-lg border border-border bg-card p-3 text-card-foreground shadow-elevation-1 transition-colors',
        'hover:bg-surface-2',
        selected && 'border-primary/40 bg-surface-2 ring-1 ring-primary/25 shadow-glow',
      )}
    >
      <div className="flex items-center gap-2">
        <button
          type="button"
          onClick={onOpen}
          className="flex min-w-0 flex-1 items-center gap-2 text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring rounded"
        >
          <FileCode2 className="h-3.5 w-3.5 shrink-0 text-primary" />
          <span className="truncate font-mono text-xs font-semibold">{s.id}</span>
          {s.hasCodeowners && (
            <Badge
              variant="feature"
              className="rounded px-1.5 py-0 font-mono text-[0.6rem] uppercase tracking-wider"
            >
              owners
            </Badge>
          )}
        </button>
        <div className="flex shrink-0 items-center gap-0.5">
          <Button
            type="button"
            size="icon"
            variant="ghost"
            className="h-6 w-6 text-muted-foreground hover:text-foreground"
            aria-label={`Move ${s.id} up`}
            data-testid={`scope-reorder-up-${s.id}`}
            disabled={!canMoveUp || reorderPending}
            onClick={onMoveUp}
          >
            <ChevronUp className="h-3.5 w-3.5" />
          </Button>
          <Button
            type="button"
            size="icon"
            variant="ghost"
            className="h-6 w-6 text-muted-foreground hover:text-foreground"
            aria-label={`Move ${s.id} down`}
            data-testid={`scope-reorder-down-${s.id}`}
            disabled={!canMoveDown || reorderPending}
            onClick={onMoveDown}
          >
            <ChevronDown className="h-3.5 w-3.5" />
          </Button>
          <Button
            type="button"
            size="icon"
            variant="ghost"
            className="h-6 w-6 text-muted-foreground hover:text-foreground"
            aria-label={`Edit ${s.id}`}
            data-testid={`scope-edit-${s.id}`}
            onClick={onEdit}
          >
            <Pencil className="h-3 w-3" />
          </Button>
          <DeleteScopeButton scope={s} />
        </div>
      </div>
      <div className="mt-1 flex items-center justify-between gap-2">
        <span className="truncate text-xs text-muted-foreground">{s.name || s.id}</span>
        {matchCount !== undefined && (
          <span
            data-testid={`scope-preview-match-${s.id}`}
            className={cn(
              'shrink-0 font-mono text-[0.625rem]',
              matchCount === 0 ? 'text-warning' : 'text-muted-foreground',
            )}
          >
            matches {matchCount} file{matchCount === 1 ? '' : 's'}
          </span>
        )}
      </div>
    </div>
  )
}

/** Delete confirm — destructive + irreversible, so it goes through AlertDialog. */
function DeleteScopeButton({ scope }: { scope: ScopeSummary }) {
  const [open, setOpen] = useState(false)
  const remove = useRemoveScope()
  return (
    <AlertDialog open={open} onOpenChange={setOpen}>
      <Button
        type="button"
        size="icon"
        variant="ghost"
        className="h-6 w-6 text-muted-foreground hover:text-destructive"
        aria-label={`Delete ${scope.id}`}
        data-testid={`scope-delete-${scope.id}`}
        onClick={() => setOpen(true)}
      >
        <Trash2 className="h-3 w-3" />
      </Button>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle className="font-mono">Delete scope “{scope.id}”?</AlertDialogTitle>
          <AlertDialogDescription>
            This removes the scope from <span className="font-mono">.firetrail/scopes.yaml</span>.
            Any per-scope profile deltas and alias bindings tied to it are dropped. This cannot be
            undone.
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>Cancel</AlertDialogCancel>
          <AlertDialogAction
            data-testid="scope-delete-confirm"
            disabled={remove.isPending}
            onClick={(e) => {
              e.preventDefault()
              remove.mutate(scope.id, { onSuccess: () => setOpen(false) })
            }}
            className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
          >
            {remove.isPending && <Loader2 className="mr-1 h-3 w-3 animate-spin" />}
            Delete
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}

/** Inline alerts for the preview's advisory warnings (zero-match, shadowing). */
function PreviewWarnings({ warnings }: { warnings: string[] }) {
  if (warnings.length === 0) return null
  return (
    <ul className="space-y-1.5">
      {warnings.map((w, i) => (
        <li
          key={`${w}-${i}`}
          data-testid="scope-preview-warning"
          className="flex items-start gap-2 rounded-md border border-warning/30 bg-warning/10 px-2.5 py-1.5 text-xs text-warning"
        >
          <AlertTriangle className="mt-0.5 h-3.5 w-3.5 shrink-0" />
          <span>{w}</span>
        </li>
      ))}
    </ul>
  )
}

function EmptyDetail({ scopes }: { scopes: ScopeSummary[] }) {
  if (scopes.length === 0) return null
  return (
    <div className="rounded-lg border border-dashed border-border px-6 py-12 text-center text-sm text-muted-foreground">
      <ChevronRight className="mx-auto mb-2 h-5 w-5" />
      Select a scope from the list to see its CODEOWNERS rules, or press
      <span className="mx-1 font-mono">New</span> to declare one.
    </div>
  )
}

/**
 * Standalone empty state (progressive disclosure). A repo with no scopes is a
 * single unit — that's the common, correct case — so this explains *why* you'd
 * add a scope rather than demanding one. Two opt-in affordances: add one by
 * hand, or scaffold candidates from the repo's package directories.
 */
function ScopeEmptyState({ onAddScope }: { onAddScope: () => void }) {
  const [scaffold, setScaffold] = useState(false)
  return (
    <div className="mx-auto max-w-2xl space-y-4 py-6" data-testid="scope-empty-state">
      <EmptyState
        icon={FolderTree}
        title="No scopes — this repo is a single unit"
        description="Scopes are only needed for a monorepo where packages need separate ownership or validation. If that's not you, you can leave this empty."
        action={
          <div className="flex flex-wrap items-center justify-center gap-2">
            <Button
              type="button"
              size="sm"
              className="gap-1.5"
              data-testid="scope-create-open"
              onClick={onAddScope}
            >
              <Plus className="h-3.5 w-3.5" />
              Add a scope
            </Button>
            <Button
              type="button"
              size="sm"
              variant="outline"
              className="gap-1.5"
              data-testid="scope-scaffold-open"
              onClick={() => setScaffold((v) => !v)}
            >
              <ListTree className="h-3.5 w-3.5" />
              Scaffold from directories
            </Button>
          </div>
        }
      />
      {scaffold && <ScaffoldHelper />}
    </div>
  )
}

/**
 * Suggest-only monorepo scaffold. Queries the file index for top-level package
 * directories (`apps/`, `packages/`, `crates/`, or first-level dirs) and lists
 * each as a *candidate* scope (id = dir, appliesTo = `<dir>/**`). The user must
 * confirm each one — nothing is ever auto-created.
 */
function ScaffoldHelper() {
  const { data, isLoading } = useFiles('', true)
  const add = useAddScope()
  const [created, setCreated] = useState<Set<string>>(new Set())

  // Prefer well-known monorepo roots; fall back to first-level directories.
  const candidates = useMemo(() => {
    const dirs = (data?.paths ?? []).map((p) => p.replace(/\/$/, '')).filter(Boolean)
    const known = dirs.filter((d) => /^(apps|packages|crates)$/.test(d))
    const pool = known.length > 0 ? known : dirs.filter((d) => !d.includes('/'))
    return Array.from(new Set(pool))
  }, [data])

  function confirm(dir: string) {
    add.mutate(
      { id: dir, appliesTo: [`${dir}/**`], aliases: [] },
      { onSuccess: () => setCreated((prev) => new Set(prev).add(dir)) },
    )
  }

  return (
    <Card className="hover:bg-card">
      <div className="space-y-3 p-4">
        <div className="space-y-1">
          <h3 className="text-sm font-medium">Candidate scopes from directories</h3>
          <p className="text-xs text-muted-foreground">
            These are suggestions only. Confirm each one you want — id maps to the
            directory and matches <span className="font-mono">{'<dir>/**'}</span>.
          </p>
        </div>
        {isLoading ? (
          <Skeleton className="h-20 w-full rounded-md" />
        ) : candidates.length === 0 ? (
          <p className="rounded-md border border-dashed border-border px-3 py-3 text-xs text-muted-foreground">
            No obvious package directories found. Add a scope by hand instead.
          </p>
        ) : (
          <ul className="space-y-1.5" data-testid="scope-scaffold-candidates">
            {candidates.map((dir) => {
              const done = created.has(dir)
              return (
                <li
                  key={dir}
                  className="flex items-center gap-2 rounded-md border border-border/60 bg-background/60 px-3 py-1.5"
                >
                  <div className="min-w-0 flex-1">
                    <div className="truncate font-mono text-xs font-medium">{dir}</div>
                    <div className="truncate font-mono text-[0.65rem] text-muted-foreground">
                      {dir}/**
                    </div>
                  </div>
                  <Button
                    type="button"
                    size="sm"
                    variant={done ? 'ghost' : 'outline'}
                    className="gap-1"
                    disabled={done || add.isPending}
                    data-testid={`scope-scaffold-confirm-${dir}`}
                    onClick={() => confirm(dir)}
                  >
                    {done ? 'Added' : (<><Plus className="h-3 w-3" />Add</>)}
                  </Button>
                </li>
              )
            })}
          </ul>
        )}
      </div>
    </Card>
  )
}

/**
 * Create / edit form. Drives both flows off one component: in edit mode the
 * fields are prefilled and the submit sends a partial `PUT`; in create mode it
 * sends a full `POST`. `appliesTo` is repeatable (one or more glob rows, each a
 * `FilePathCombobox`), with a "glob from directory" affordance that turns a
 * picked directory into `<dir>/**`. Aliases are chips. Validation errors from
 * the API surface as a toast (via the mutation hook) and an inline banner.
 */
function ScopeForm({ state, onClose }: { state: { mode: 'create' } | { mode: 'edit'; scope: ScopeSummary }; onClose: () => void }) {
  const editing = state.mode === 'edit' ? state.scope : null
  const add = useAddScope()
  const edit = useEditScope()
  const pending = add.isPending || edit.isPending

  const [id, setId] = useState(editing?.id ?? '')
  const [name, setName] = useState(editing?.name ?? '')
  const [globs, setGlobs] = useState<string[]>(
    editing && editing.appliesTo.length > 0 ? editing.appliesTo : [''],
  )
  const [aliases, setAliases] = useState<string[]>(editing?.aliases ?? [])
  const [aliasDraft, setAliasDraft] = useState('')
  const [codeowners, setCodeowners] = useState('')
  const [dirDraft, setDirDraft] = useState('')
  const [error, setError] = useState<string | null>(null)

  function setGlob(i: number, v: string) {
    setGlobs((prev) => prev.map((g, idx) => (idx === i ? v : g)))
  }
  function addGlobRow() {
    setGlobs((prev) => [...prev, ''])
  }
  function removeGlobRow(i: number) {
    setGlobs((prev) => (prev.length <= 1 ? prev : prev.filter((_, idx) => idx !== i)))
  }
  function addGlobFromDir() {
    const dir = dirDraft.trim().replace(/\/+$/, '')
    if (!dir) return
    const glob = `${dir}/**`
    setGlobs((prev) => {
      const cleaned = prev.filter((g) => g.trim() !== '')
      return cleaned.includes(glob) ? cleaned : [...cleaned, glob]
    })
    setDirDraft('')
  }

  function addAlias() {
    const a = aliasDraft.trim()
    if (!a || aliases.includes(a)) {
      setAliasDraft('')
      return
    }
    setAliases((prev) => [...prev, a])
    setAliasDraft('')
  }
  function removeAlias(a: string) {
    setAliases((prev) => prev.filter((x) => x !== a))
  }

  function submit() {
    setError(null)
    const cleanGlobs = globs.map((g) => g.trim()).filter(Boolean)
    const trimmedId = id.trim()
    const owners = codeowners.trim()
    const nm = name.trim()

    if (!editing && !trimmedId) {
      setError('A scope id is required.')
      return
    }
    if (cleanGlobs.length === 0) {
      setError('Add at least one applies-to glob.')
      return
    }

    if (editing) {
      edit.mutate(
        {
          id: editing.id,
          input: {
            name: nm || null,
            appliesTo: cleanGlobs,
            aliases,
            ...(owners ? { codeowners: owners } : {}),
          },
        },
        { onSuccess: onClose },
      )
    } else {
      add.mutate(
        {
          id: trimmedId,
          name: nm || null,
          appliesTo: cleanGlobs,
          aliases,
          codeowners: owners || null,
        },
        { onSuccess: onClose },
      )
    }
  }

  return (
    <Card className="hover:bg-card">
      <form
        className="space-y-5 p-4"
        onSubmit={(e) => {
          e.preventDefault()
          submit()
        }}
      >
        <header className="flex items-center justify-between">
          <h2 className="font-display text-lg font-semibold tracking-tight">
            {editing ? `Edit scope “${editing.id}”` : 'New scope'}
          </h2>
          <Button
            type="button"
            size="icon"
            variant="ghost"
            className="h-7 w-7"
            aria-label="Close form"
            onClick={onClose}
          >
            <X className="h-4 w-4" />
          </Button>
        </header>

        {error && (
          <p className="flex items-start gap-2 rounded-md border border-destructive/40 bg-destructive/5 px-3 py-2 text-xs text-destructive">
            <AlertTriangle className="mt-0.5 h-3.5 w-3.5 shrink-0" />
            {error}
          </p>
        )}

        <div className="grid gap-4 sm:grid-cols-2">
          <div className="space-y-1.5">
            <Label htmlFor="scope-form-id">Id</Label>
            <Input
              id="scope-form-id"
              data-testid="scope-form-id"
              value={id}
              onChange={(e) => setId(e.target.value)}
              placeholder="e.g. ft-core"
              disabled={!!editing}
              className="font-mono text-xs"
              autoFocus={!editing}
            />
            {editing && (
              <p className="text-[0.65rem] text-muted-foreground">
                Ids are immutable; create a new scope to rename.
              </p>
            )}
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="scope-form-name">Name (optional)</Label>
            <Input
              id="scope-form-name"
              data-testid="scope-form-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="Display name"
            />
          </div>
        </div>

        <div className="space-y-2">
          <Label>Applies to (globs)</Label>
          <div className="space-y-2">
            {globs.map((g, i) => (
              <div key={i} className="flex items-center gap-2">
                <div className="flex-1">
                  <FilePathCombobox
                    value={g}
                    onValueChange={(v) => setGlob(i, v)}
                    placeholder="e.g. crates/ft-core/** (free-typed globs ok)"
                    className="font-mono text-xs"
                    data-testid={i === 0 ? 'scope-form-applies-to' : `scope-form-applies-to-${i}`}
                  />
                </div>
                <Button
                  type="button"
                  size="icon"
                  variant="ghost"
                  className="h-8 w-8 shrink-0 text-muted-foreground hover:text-destructive"
                  aria-label={`Remove glob ${i + 1}`}
                  disabled={globs.length <= 1}
                  onClick={() => removeGlobRow(i)}
                >
                  <X className="h-3.5 w-3.5" />
                </Button>
              </div>
            ))}
          </div>
          <Button
            type="button"
            size="sm"
            variant="outline"
            className="gap-1"
            data-testid="scope-form-add-glob"
            onClick={addGlobRow}
          >
            <Plus className="h-3.5 w-3.5" />
            Add glob
          </Button>

          <div className="flex items-end gap-2 rounded-md border border-dashed border-border/60 p-2">
            <div className="flex-1 space-y-1">
              <Label className="text-[0.65rem] uppercase tracking-wide text-muted-foreground">
                Glob from a directory
              </Label>
              <FilePathCombobox
                dirs
                value={dirDraft}
                onValueChange={setDirDraft}
                placeholder="pick a directory → adds <dir>/**"
                className="font-mono text-xs"
                data-testid="scope-form-dir-glob"
              />
            </div>
            <Button
              type="button"
              size="sm"
              variant="outline"
              onClick={addGlobFromDir}
              disabled={dirDraft.trim() === ''}
              data-testid="scope-form-dir-glob-add"
            >
              Add as glob
            </Button>
          </div>
        </div>

        <div className="space-y-2">
          <Label>Aliases</Label>
          {aliases.length > 0 && (
            <div className="flex flex-wrap gap-1.5">
              {aliases.map((a) => (
                <span
                  key={a}
                  className="inline-flex items-center gap-1 rounded-full bg-muted px-2.5 py-0.5 font-mono text-xs"
                  data-testid={`scope-form-alias-${a}`}
                >
                  {a}
                  <button
                    type="button"
                    aria-label={`Remove alias ${a}`}
                    className="text-muted-foreground hover:text-destructive"
                    onClick={() => removeAlias(a)}
                  >
                    <X className="h-3 w-3" />
                  </button>
                </span>
              ))}
            </div>
          )}
          <div className="flex items-center gap-2">
            <Input
              value={aliasDraft}
              onChange={(e) => setAliasDraft(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') {
                  e.preventDefault()
                  addAlias()
                }
              }}
              placeholder="alias (Enter to add)"
              className="font-mono text-xs"
              data-testid="scope-form-alias-input"
            />
            <Button
              type="button"
              size="sm"
              variant="outline"
              onClick={addAlias}
              disabled={aliasDraft.trim() === ''}
              data-testid="scope-form-alias-add"
            >
              Add
            </Button>
          </div>
        </div>

        <div className="space-y-1.5">
          <Label>CODEOWNERS path (optional)</Label>
          <FilePathCombobox
            value={codeowners}
            onValueChange={setCodeowners}
            placeholder="e.g. .github/CODEOWNERS"
            className="font-mono text-xs"
            data-testid="scope-form-codeowners"
          />
          {editing && (
            <p className="text-[0.65rem] text-muted-foreground">
              Leave blank to keep the current CODEOWNERS wiring unchanged.
            </p>
          )}
        </div>

        <div className="flex justify-end gap-2">
          <Button type="button" variant="ghost" onClick={onClose} disabled={pending}>
            Cancel
          </Button>
          <Button type="submit" disabled={pending} data-testid="scope-form-submit">
            {pending && <Loader2 className="mr-1 h-3 w-3 animate-spin" />}
            {editing ? 'Save changes' : 'Create scope'}
          </Button>
        </div>
      </form>
    </Card>
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

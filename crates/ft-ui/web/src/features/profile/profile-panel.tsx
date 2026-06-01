/**
 * Repo **Profile** panel (RepoProfile epic).
 *
 * Surfaces the singleton `RepoProfile` record: the validate/test/build/lint
 * commands, tooling facts (languages, package managers, runtime), and a shallow
 * component map. Every field is editable inline and persists through the
 * partial-update `PUT /api/profile` (same semantics as `firetrail profile set`).
 * Components are added/removed via the dedicated endpoints.
 *
 * Trust (Draft → Reviewed → Verified) reuses the existing `/api/trust/*` routes
 * and the shared `TrustBadge` — this panel only offers the two forward steps
 * relevant to confirming a profile; the full trust state machine lives in the
 * memory surface.
 */
import { createContext, useContext, useEffect, useState } from 'react'
import {
  Boxes,
  Check,
  Layers,
  Loader2,
  Pencil,
  Plus,
  ShieldCheck,
  Terminal,
  Trash2,
  X,
} from 'lucide-react'
import { useQueryClient } from '@tanstack/react-query'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { FilePathCombobox } from '@/components/ui/autocomplete'
import { Textarea } from '@/components/ui/textarea'
import { Label } from '@/components/ui/label'
import { Skeleton } from '@/components/ui/skeleton'
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from '@/components/ui/card'
import { ApiError, toastApiError } from '@/api/error'
import { useEvents } from '@/api/hooks/useEvents'
import type { Event as AppEvent } from '@/api/types/Event'
import type { ProfileView } from '@/api/types/ProfileView'
import { TrustBadge } from '@/features/trust/trust-badge'
import { postPromote, postReview } from '@/features/trust/api'
import {
  profileKey,
  useAddComponent,
  useProfileQuery,
  useRemoveComponent,
  useScopesQuery,
  useUpdateProfile,
} from './use-profile-query'
import type { ProfilePatch, ProfileSelector } from './api'

/** The base profile (no scope selected). */
const BASE = '__base__'

/**
 * The active scope selector, shared with the inline editors so each `Save`
 * writes to the right record (base vs. a per-scope delta). A resolved view is
 * read-only — its editors are disabled.
 */
const SelectorContext = createContext<{ selector: ProfileSelector; readOnly: boolean }>({
  selector: {},
  readOnly: false,
})

export function ProfilePanel() {
  const qc = useQueryClient()
  const [scopeSel, setScopeSel] = useState<string>(BASE)
  const [resolved, setResolved] = useState(false)

  const isBase = scopeSel === BASE
  const selector: ProfileSelector = isBase ? {} : { scope: scopeSel, resolved }
  const readOnly = !isBase && resolved

  const { data: scopes = [] } = useScopesQuery()
  const { data, isLoading, error } = useProfileQuery(selector)

  // Re-fetch when another client edits the profile (any selector under the key).
  const { last } = useEvents<AppEvent>({})
  useEffect(() => {
    if (last && last.kind === 'profile_updated') {
      void qc.invalidateQueries({ queryKey: profileKey })
    }
  }, [last, qc])

  return (
    <div className="mx-auto max-w-3xl space-y-6 p-6">
      <header className="space-y-1">
        <h1 className="font-display text-xl font-semibold tracking-tight">Repo profile</h1>
        <p className="text-sm text-muted-foreground">
          The always-read facts firetrail keeps about this repo — validate command, standard
          commands, tooling, and the component map. Edits are saved immediately and return the
          profile to <span className="font-mono">draft</span> until re-confirmed.
        </p>
      </header>

      <ScopeSwitcher
        scopes={scopes}
        value={scopeSel}
        onValueChange={(v) => {
          setScopeSel(v)
          if (v === BASE) setResolved(false)
        }}
        resolved={resolved}
        onResolvedChange={setResolved}
      />

      {isLoading ? (
        <Skeleton className="h-64 w-full" />
      ) : error ? (
        <p className="rounded-md border border-destructive/40 bg-destructive/5 px-3 py-3 text-sm text-destructive">
          Failed to load profile: {(error as Error).message}
        </p>
      ) : (
        <SelectorContext.Provider value={{ selector, readOnly }}>
          <ProfileBody profile={data ?? null} showScopeTrust={!isBase} />
        </SelectorContext.Provider>
      )}
    </div>
  )
}

/** Base/scope switcher + a Resolved toggle (only meaningful for a scope). */
function ScopeSwitcher({
  scopes,
  value,
  onValueChange,
  resolved,
  onResolvedChange,
}: {
  scopes: { id: string; name: string }[]
  value: string
  onValueChange: (v: string) => void
  resolved: boolean
  onResolvedChange: (v: boolean) => void
}) {
  const isScope = value !== BASE
  return (
    <div className="flex flex-wrap items-center gap-3 rounded-lg border border-border bg-card/40 px-4 py-3">
      <Layers className="h-4 w-4 text-muted-foreground" />
      <Label className="text-xs uppercase tracking-wide text-muted-foreground" htmlFor="scope-sel">
        Scope
      </Label>
      <select
        id="scope-sel"
        data-testid="profile-scope-switcher"
        value={value}
        onChange={(e) => onValueChange(e.target.value)}
        className="h-8 rounded-md border border-border bg-background px-2 text-sm"
      >
        <option value={BASE}>Base (repo-wide)</option>
        {scopes.map((s) => (
          <option key={s.id} value={s.id}>
            {s.name}
          </option>
        ))}
      </select>

      <label
        className={
          'ml-auto flex items-center gap-2 text-sm ' +
          (isScope ? 'cursor-pointer' : 'cursor-not-allowed opacity-50')
        }
      >
        <input
          type="checkbox"
          data-testid="profile-resolved-toggle"
          checked={resolved}
          disabled={!isScope}
          onChange={(e) => onResolvedChange(e.target.checked)}
        />
        Resolved
      </label>
    </div>
  )
}

function ProfileBody({
  profile,
  showScopeTrust,
}: {
  profile: ProfileView | null
  showScopeTrust: boolean
}) {
  return (
    <div className="space-y-6">
      {showScopeTrust ? <ScopeTrustRow profile={profile} /> : <TrustRow profile={profile} />}

      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-base">
            <Terminal className="h-4 w-4" />
            Commands
          </CardTitle>
          <CardDescription>
            The validate command is the canonical &ldquo;prove a change is good&rdquo; gate the
            audit loop runs.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <EditableField
            label="Validate"
            value={profile?.validate_command ?? null}
            field="validateCommand"
            placeholder="e.g. cargo fmt --check && cargo test && cargo clippy"
            mono
          />
          <EditableField label="Test" value={profile?.test_command ?? null} field="testCommand" mono />
          <EditableField
            label="Build"
            value={profile?.build_command ?? null}
            field="buildCommand"
            mono
          />
          <EditableField label="Lint" value={profile?.lint_command ?? null} field="lintCommand" mono />
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="text-base">Tooling</CardTitle>
          <CardDescription>Languages, package managers, and runtime.</CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <EditableListField label="Languages" value={profile?.languages ?? []} field="languages" />
          <EditableListField
            label="Package managers"
            value={profile?.package_managers ?? []}
            field="packageManagers"
          />
          <EditableField label="Runtime" value={profile?.runtime ?? null} field="runtime" />
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-base">
            <Boxes className="h-4 w-4" />
            Components
          </CardTitle>
          <CardDescription>A shallow map of the repo&rsquo;s areas (name + path).</CardDescription>
        </CardHeader>
        <CardContent>
          <ComponentMap profile={profile} />
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="text-base">Notes</CardTitle>
        </CardHeader>
        <CardContent>
          <EditableField
            label="Notes"
            value={profile?.notes ?? null}
            field="notes"
            multiline
            hideLabel
            placeholder="Free-form notes about the repo…"
          />
        </CardContent>
      </Card>
    </div>
  )
}

/** Trust badge + the two forward confirmation steps, wired to /api/trust/*. */
function TrustRow({ profile }: { profile: ProfileView | null }) {
  const qc = useQueryClient()
  const [pending, setPending] = useState<'review' | 'promote' | null>(null)
  const trust = profile?.trust ?? null
  const id = profile?.id

  function run(op: 'review' | 'promote') {
    if (!id) return
    setPending(op)
    const call = op === 'review' ? postReview(id) : postPromote(id)
    void call
      .then(() => {
        toast.success(op === 'review' ? 'Marked reviewed' : 'Marked verified')
        void qc.invalidateQueries({ queryKey: profileKey })
      })
      .catch((err) => {
        if (err instanceof ApiError && err.kind === 'conflict') {
          toast.error('Wrong source state — refresh and try again')
          void qc.invalidateQueries({ queryKey: profileKey })
          return
        }
        toastApiError(err)
      })
      .finally(() => setPending(null))
  }

  return (
    <div className="flex flex-wrap items-center gap-3 rounded-lg border border-border bg-card/40 px-4 py-3">
      <span className="text-xs font-medium uppercase tracking-wide text-muted-foreground">
        Trust
      </span>
      <TrustBadge state={trust} />
      <div className="ml-auto flex gap-2">
        <Button
          size="sm"
          variant="outline"
          disabled={!id || trust === 'reviewed' || trust === 'verified' || pending !== null}
          onClick={() => run('review')}
          data-testid="profile-review"
        >
          {pending === 'review' ? (
            <Loader2 className="mr-1 h-3.5 w-3.5 animate-spin" />
          ) : (
            <Check className="mr-1 h-3.5 w-3.5" />
          )}
          Mark reviewed
        </Button>
        <Button
          size="sm"
          disabled={!id || trust !== 'reviewed' || pending !== null}
          onClick={() => run('promote')}
          data-testid="profile-verify"
        >
          {pending === 'promote' ? (
            <Loader2 className="mr-1 h-3.5 w-3.5 animate-spin" />
          ) : (
            <ShieldCheck className="mr-1 h-3.5 w-3.5" />
          )}
          Verify
        </Button>
      </div>
    </div>
  )
}

/**
 * The trust badge for a per-scope delta (read-only). The forward confirmation
 * steps live on the base `TrustRow`; a scope delta only surfaces its own state.
 */
function ScopeTrustRow({ profile }: { profile: ProfileView | null }) {
  return (
    <div className="flex flex-wrap items-center gap-3 rounded-lg border border-border bg-card/40 px-4 py-3">
      <span className="text-xs font-medium uppercase tracking-wide text-muted-foreground">
        Scope trust
      </span>
      <span data-testid="profile-scope-trust">
        <TrustBadge state={profile?.trust ?? null} />
      </span>
    </div>
  )
}

/** A single string field, edited inline and persisted via PUT /api/profile. */
function EditableField({
  label,
  value,
  field,
  placeholder,
  mono,
  multiline,
  hideLabel,
}: {
  label: string
  value: string | null
  field: keyof ProfilePatch
  placeholder?: string
  mono?: boolean
  multiline?: boolean
  hideLabel?: boolean
}) {
  const { selector, readOnly } = useContext(SelectorContext)
  const [editing, setEditing] = useState(false)
  const [draft, setDraft] = useState(value ?? '')
  const update = useUpdateProfile(selector)

  function save() {
    const next = draft.trim() === '' ? null : draft
    update.mutate({ [field]: next } as ProfilePatch, { onSuccess: () => setEditing(false) })
  }

  // A resolved (merged) view is read-only — edits would be ambiguous.
  if (readOnly) {
    return (
      <div className="space-y-1.5">
        {!hideLabel && (
          <Label className="text-xs uppercase tracking-wide text-muted-foreground">{label}</Label>
        )}
        <div
          className={
            'w-full rounded-md border border-border/40 px-3 py-2 text-sm ' +
            (value ? '' : 'text-muted-foreground') +
            (mono && value ? ' font-mono text-xs' : '')
          }
          data-testid={`profile-value-${field}`}
        >
          {value ?? 'Not set'}
        </div>
      </div>
    )
  }

  return (
    <div className="space-y-1.5">
      {!hideLabel && (
        <div className="flex items-center justify-between">
          <Label className="text-xs uppercase tracking-wide text-muted-foreground">{label}</Label>
          {!editing && (
            <Button
              type="button"
              size="sm"
              variant="ghost"
              className="h-6 gap-1 px-2 text-xs"
              data-testid={`profile-edit-${field}`}
              onClick={() => {
                setDraft(value ?? '')
                setEditing(true)
              }}
            >
              <Pencil className="h-3 w-3" />
              Edit
            </Button>
          )}
        </div>
      )}

      {editing ? (
        <div className="space-y-2">
          {multiline ? (
            <Textarea value={draft} onChange={(e) => setDraft(e.target.value)} rows={4} autoFocus />
          ) : (
            <Input
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
              placeholder={placeholder}
              className={mono ? 'font-mono text-xs' : undefined}
              autoFocus
            />
          )}
          <div className="flex justify-end gap-2">
            <Button
              type="button"
              size="sm"
              variant="ghost"
              onClick={() => setEditing(false)}
              disabled={update.isPending}
            >
              Cancel
            </Button>
            <Button
              type="button"
              size="sm"
              onClick={save}
              disabled={update.isPending}
              data-testid={`profile-save-${field}`}
            >
              {update.isPending && <Loader2 className="mr-1 h-3 w-3 animate-spin" />}
              Save
            </Button>
          </div>
        </div>
      ) : (
        <button
          type="button"
          onClick={() => {
            setDraft(value ?? '')
            setEditing(true)
          }}
          className={
            'w-full rounded-md border border-dashed border-border/60 px-3 py-2 text-left text-sm transition-colors hover:border-border ' +
            (value ? '' : 'text-muted-foreground') +
            (mono && value ? ' font-mono text-xs' : '')
          }
          data-testid={`profile-value-${field}`}
        >
          {value ?? placeholder ?? 'Not set — click to add'}
        </button>
      )}
    </div>
  )
}

/** A comma/whitespace-separated list field (languages, package managers). */
function EditableListField({
  label,
  value,
  field,
}: {
  label: string
  value: string[]
  field: 'languages' | 'packageManagers'
}) {
  const { selector, readOnly } = useContext(SelectorContext)
  const [editing, setEditing] = useState(false)
  const [draft, setDraft] = useState(value.join(', '))
  const update = useUpdateProfile(selector)

  function save() {
    const next = draft
      .split(/[\s,]+/)
      .map((s) => s.trim())
      .filter(Boolean)
    update.mutate({ [field]: next } as ProfilePatch, { onSuccess: () => setEditing(false) })
  }

  if (readOnly) {
    return (
      <div className="space-y-1.5">
        <Label className="text-xs uppercase tracking-wide text-muted-foreground">{label}</Label>
        {value.length === 0 ? (
          <div className="text-sm text-muted-foreground" data-testid={`profile-value-${field}`}>
            None
          </div>
        ) : (
          <div className="flex flex-wrap gap-1.5" data-testid={`profile-value-${field}`}>
            {value.map((v) => (
              <span
                key={v}
                className="rounded-full bg-muted px-2.5 py-0.5 font-mono text-xs text-foreground"
              >
                {v}
              </span>
            ))}
          </div>
        )}
      </div>
    )
  }

  return (
    <div className="space-y-1.5">
      <div className="flex items-center justify-between">
        <Label className="text-xs uppercase tracking-wide text-muted-foreground">{label}</Label>
        {!editing && (
          <Button
            type="button"
            size="sm"
            variant="ghost"
            className="h-6 gap-1 px-2 text-xs"
            data-testid={`profile-edit-${field}`}
            onClick={() => {
              setDraft(value.join(', '))
              setEditing(true)
            }}
          >
            <Pencil className="h-3 w-3" />
            Edit
          </Button>
        )}
      </div>

      {editing ? (
        <div className="space-y-2">
          <Input
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            placeholder="comma or space separated"
            autoFocus
          />
          <div className="flex justify-end gap-2">
            <Button
              type="button"
              size="sm"
              variant="ghost"
              onClick={() => setEditing(false)}
              disabled={update.isPending}
            >
              Cancel
            </Button>
            <Button
              type="button"
              size="sm"
              onClick={save}
              disabled={update.isPending}
              data-testid={`profile-save-${field}`}
            >
              {update.isPending && <Loader2 className="mr-1 h-3 w-3 animate-spin" />}
              Save
            </Button>
          </div>
        </div>
      ) : value.length === 0 ? (
        <button
          type="button"
          onClick={() => setEditing(true)}
          className="w-full rounded-md border border-dashed border-border/60 px-3 py-2 text-left text-sm text-muted-foreground hover:border-border"
          data-testid={`profile-value-${field}`}
        >
          None — click to add
        </button>
      ) : (
        <div className="flex flex-wrap gap-1.5" data-testid={`profile-value-${field}`}>
          {value.map((v) => (
            <span
              key={v}
              className="rounded-full bg-muted px-2.5 py-0.5 font-mono text-xs text-foreground"
            >
              {v}
            </span>
          ))}
        </div>
      )}
    </div>
  )
}

function ComponentMap({ profile }: { profile: ProfileView | null }) {
  const remove = useRemoveComponent()
  const components = profile?.components ?? []

  return (
    <div className="space-y-3">
      {components.length === 0 ? (
        <p className="rounded-md border border-dashed border-border/60 px-3 py-3 text-sm text-muted-foreground">
          No components mapped yet.
        </p>
      ) : (
        <ul className="space-y-2" data-testid="profile-components">
          {components.map((c) => (
            <li
              key={c.name}
              className="flex items-center gap-3 rounded-md border border-border/50 bg-background/60 px-3 py-2"
            >
              <div className="min-w-0 flex-1">
                <div className="truncate text-sm font-medium">{c.name}</div>
                <div className="truncate font-mono text-[0.65rem] text-muted-foreground">
                  {c.path}
                </div>
                {c.summary && (
                  <div className="truncate text-xs text-muted-foreground">{c.summary}</div>
                )}
              </div>
              <Button
                type="button"
                size="icon"
                variant="ghost"
                className="h-7 w-7 shrink-0 text-muted-foreground hover:text-destructive"
                aria-label={`Remove ${c.name}`}
                data-testid={`profile-component-remove-${c.name}`}
                disabled={remove.isPending}
                onClick={() => remove.mutate(c.name)}
              >
                <Trash2 className="h-3.5 w-3.5" />
              </Button>
            </li>
          ))}
        </ul>
      )}
      <AddComponentForm />
    </div>
  )
}

function AddComponentForm() {
  const [open, setOpen] = useState(false)
  const [name, setName] = useState('')
  const [path, setPath] = useState('')
  const [summary, setSummary] = useState('')
  const add = useAddComponent()

  function submit() {
    if (!name.trim() || !path.trim()) return
    add.mutate(
      { name: name.trim(), path: path.trim(), summary: summary.trim() || undefined },
      {
        onSuccess: () => {
          setName('')
          setPath('')
          setSummary('')
          setOpen(false)
        },
      },
    )
  }

  if (!open) {
    return (
      <Button
        type="button"
        size="sm"
        variant="outline"
        className="gap-1.5"
        data-testid="profile-component-add-open"
        onClick={() => setOpen(true)}
      >
        <Plus className="h-3.5 w-3.5" />
        Add component
      </Button>
    )
  }

  return (
    <div className="space-y-2 rounded-md border border-border/60 bg-background/60 p-3">
      <div className="grid gap-2 sm:grid-cols-2">
        <Input
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="name (e.g. ft-cli)"
          autoFocus
          data-testid="profile-component-name"
        />
        <FilePathCombobox
          dirs
          value={path}
          onValueChange={setPath}
          placeholder="path (e.g. crates/ft-cli)"
          className="font-mono text-xs"
          data-testid="profile-component-path"
        />
      </div>
      <Input
        value={summary}
        onChange={(e) => setSummary(e.target.value)}
        placeholder="summary (optional)"
        data-testid="profile-component-summary"
      />
      <div className="flex justify-end gap-2">
        <Button
          type="button"
          size="sm"
          variant="ghost"
          className="gap-1"
          onClick={() => setOpen(false)}
          disabled={add.isPending}
        >
          <X className="h-3 w-3" />
          Cancel
        </Button>
        <Button
          type="button"
          size="sm"
          onClick={submit}
          disabled={!name.trim() || !path.trim() || add.isPending}
          data-testid="profile-component-add-submit"
        >
          {add.isPending && <Loader2 className="mr-1 h-3 w-3 animate-spin" />}
          Add
        </Button>
      </div>
    </div>
  )
}

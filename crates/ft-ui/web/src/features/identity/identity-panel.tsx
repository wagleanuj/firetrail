/**
 * Identity browser.
 *
 * Top filter bar (kind, status). Left list, right detail with a capability
 * matrix. "+ Register" opens the create dialog; "Offboard" opens a typed
 * confirmation.
 */
import { useState } from 'react'
import { useNavigate } from '@tanstack/react-router'
import { Plus, UserX, ShieldCheck, ShieldX, Users } from 'lucide-react'
import { EmptyState } from '@/components/ui/empty-state'
import type { IdentityKindInput } from '@/api/types/IdentityKindInput'
import type { IdentityStatusFilter } from '@/api/types/IdentityStatusFilter'
import type { IdentityView } from '@/api/types/IdentityView'
import { Button } from '@/components/ui/button'
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
import { useIdentityList, useIdentity } from './use-identity-query'
import { CapabilityMatrix } from './capability-matrix'
import { CapabilityEditor } from './capability-editor'
import { RegisterIdentityDialog, OffboardConfirmDialog } from './register-dialog'

const KIND_OPTIONS: IdentityKindInput[] = ['human', 'bot', 'ci']
const STATUS_OPTIONS: IdentityStatusFilter[] = ['active', 'offboarded']

interface IdentityPanelProps {
  selectedId?: string
}

export function IdentityPanel({ selectedId }: IdentityPanelProps) {
  const navigate = useNavigate()
  const [kind, setKind] = useState<IdentityKindInput | undefined>()
  const [status, setStatus] = useState<IdentityStatusFilter | undefined>()
  const [registerOpen, setRegisterOpen] = useState(false)

  const list = useIdentityList({ kind: kind ?? null, status: status ?? null })

  return (
    <div className="mx-auto flex h-full max-w-6xl flex-col gap-4 p-6">
      <header className="flex items-start justify-between gap-4">
        <div>
          <h1 className="font-mono text-lg font-semibold tracking-tight">Identity</h1>
          <p className="text-sm text-muted-foreground">
            Humans, bots, and CI service accounts — plus their effective capabilities.
          </p>
        </div>
        <Button onClick={() => setRegisterOpen(true)} size="sm" className="gap-2">
          <Plus className="h-4 w-4" />
          Register
        </Button>
      </header>

      <div className="flex items-end gap-3 rounded-md border border-border/70 bg-background/60 p-3">
        <div className="space-y-1.5">
          <Label className="text-xs">Kind</Label>
          <Select
            value={kind ?? '__all__'}
            onValueChange={(v) => setKind(v === '__all__' ? undefined : (v as IdentityKindInput))}
          >
            <SelectTrigger className="w-32">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="__all__">All kinds</SelectItem>
              {KIND_OPTIONS.map((k) => (
                <SelectItem key={k} value={k}>
                  {k}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <div className="space-y-1.5">
          <Label className="text-xs">Status</Label>
          <Select
            value={status ?? '__any__'}
            onValueChange={(v) => setStatus(v === '__any__' ? undefined : (v as IdentityStatusFilter))}
          >
            <SelectTrigger className="w-36">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="__any__">Any status</SelectItem>
              {STATUS_OPTIONS.map((s) => (
                <SelectItem key={s} value={s}>
                  {s}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      </div>

      <div className="grid flex-1 grid-cols-1 gap-4 lg:grid-cols-[18rem_1fr]">
        <aside className="flex flex-col gap-2">
          {list.isLoading && (
            <div className="space-y-1.5">
              {Array.from({ length: 6 }).map((_, i) => (
                <Skeleton key={i} className="h-14 w-full" />
              ))}
            </div>
          )}
          {list.error && (
            <p className="text-sm text-destructive">
              Failed to load identities: {(list.error as Error).message}
            </p>
          )}
          <ul data-testid="identity-list" className="flex flex-col gap-1 overflow-y-auto">
            {list.data?.identities.map((idn) => (
              <li key={idn.id}>
                <button
                  type="button"
                  onClick={() => navigate({ to: '/identity/$id', params: { id: idn.id } })}
                  className={cn(
                    'group w-full rounded-md border border-border/70 bg-background/80 px-3 py-2 text-left transition-all',
                    'hover:-translate-y-0.5 hover:border-primary/40',
                    selectedId === idn.id &&
                      'border-primary/60 bg-primary/5 shadow-[0_0_0_1px_hsl(var(--primary)/0.25)]',
                  )}
                >
                  <div className="flex items-center gap-2">
                    <KindPill kind={idn.kind} />
                    <span className="font-mono text-xs font-semibold">{idn.id}</span>
                    <StatusDot status={idn.status} />
                  </div>
                  <div className="mt-0.5 truncate text-xs text-muted-foreground">
                    {idn.emails[0] ?? idn.name}
                  </div>
                </button>
              </li>
            ))}
            {list.data && list.data.identities.length === 0 && (
              <li>
                <EmptyState
                  icon={Users}
                  title="No identities"
                  description={
                    kind || status
                      ? 'No identities match the current filters.'
                      : 'Register a human, bot, or CI service account to get started.'
                  }
                  action={
                    !kind && !status ? (
                      <Button size="sm" onClick={() => setRegisterOpen(true)} className="gap-2">
                        <Plus className="h-4 w-4" />
                        Register
                      </Button>
                    ) : undefined
                  }
                />
              </li>
            )}
          </ul>
        </aside>

        <section>
          {selectedId ? (
            <IdentityDetail id={selectedId} />
          ) : (
            <div className="rounded-md border border-dashed border-border/60 px-6 py-12 text-center text-sm text-muted-foreground">
              Select an identity to inspect their capability matrix.
            </div>
          )}
        </section>
      </div>

      <RegisterIdentityDialog open={registerOpen} onOpenChange={setRegisterOpen} />
    </div>
  )
}

function IdentityDetail({ id }: { id: string }) {
  const { data, isLoading, error } = useIdentity(id)
  const [offboardOpen, setOffboardOpen] = useState(false)

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
        Failed to load identity: {(error as Error).message}
      </p>
    )
  }
  if (!data) return null
  const { identity } = data
  const active = identity.status === 'active'

  return (
    <div className="space-y-5">
      <header className="space-y-2">
        <div className="flex items-center gap-2 font-mono text-xs uppercase tracking-wider text-muted-foreground">
          <span>identity</span>
          <span>/</span>
          <span>{identity.id}</span>
        </div>
        <div className="flex flex-wrap items-center gap-3">
          <KindPill kind={identity.kind} />
          <h2 className="text-xl font-semibold">{identity.name}</h2>
          <StatusDot status={identity.status} />
        </div>
        <div className="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
          {identity.emails.map((e) => (
            <span key={e} className="rounded bg-muted/60 px-1.5 py-0.5 font-mono">
              {e}
            </span>
          ))}
          {identity.machines.map((m) => (
            <span key={m} className="rounded bg-muted/60 px-1.5 py-0.5 font-mono">
              {m}
            </span>
          ))}
        </div>
        <div className="pt-1">
          <Button
            size="sm"
            variant="outline"
            disabled={!active}
            onClick={() => setOffboardOpen(true)}
            className="gap-2"
          >
            <UserX className="h-3.5 w-3.5" />
            Offboard
          </Button>
        </div>
      </header>

      <section className="space-y-2">
        <h3 className="font-mono text-xs uppercase tracking-wider text-muted-foreground">
          Capability matrix
        </h3>
        <CapabilityMatrix identity={identity.id} />
      </section>

      {active && (
        <section>
          <CapabilityEditor identity={identity.id} />
        </section>
      )}

      <OffboardConfirmDialog id={identity.id} open={offboardOpen} onOpenChange={setOffboardOpen} />
    </div>
  )
}

function KindPill({ kind }: { kind: string }) {
  return (
    <span className="rounded-sm bg-primary/15 px-1.5 py-0.5 font-mono text-[0.625rem] font-semibold uppercase tracking-wider text-primary">
      {kind}
    </span>
  )
}

function StatusDot({ status }: { status: string }) {
  const active = status === 'active'
  const Icon = active ? ShieldCheck : ShieldX
  return (
    <span
      className={cn(
        'inline-flex items-center gap-1 font-mono text-[0.625rem] uppercase tracking-wider',
        active ? 'text-primary' : 'text-muted-foreground',
      )}
    >
      <Icon className="h-3 w-3" />
      {status}
    </span>
  )
}

// Suppress an unused-import diagnostic when this file is consumed via the
// route shell where `IdentityView` is referenced only for type inference.
export type _Touch = IdentityView

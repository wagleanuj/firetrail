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
import { AnimatePresence, motion, useReducedMotion } from 'framer-motion'
import { LIST_STAGGER, ROUTE_TRANSITION, reducedTransition } from '@/lib/motion'
import { PageHeader } from '@/components/page-header'
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
  const reduced = useReducedMotion() ?? false
  const transition = reducedTransition(reduced, ROUTE_TRANSITION)

  return (
    <div className="mx-auto flex h-full max-w-6xl flex-col gap-5 px-6 py-5">
      <PageHeader
        title="Identity"
        subtitle="Humans, bots, and CI service accounts — plus their effective capabilities."
        actions={
          <Button onClick={() => setRegisterOpen(true)} size="sm" className="gap-2">
            <Plus className="h-4 w-4" />
            Register
          </Button>
        }
      />

      <div className="flex flex-wrap items-end gap-3 rounded-lg border border-border bg-surface-1 p-3 shadow-elevation-1">
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

      <div className="grid flex-1 grid-cols-1 gap-5 lg:grid-cols-[19rem_1fr]">
        <aside className="flex min-h-0 flex-col gap-2.5">
          {list.isLoading && (
            <div className="space-y-2.5">
              {Array.from({ length: 6 }).map((_, i) => (
                <Skeleton key={i} className="h-[3.75rem] w-full rounded-lg" />
              ))}
            </div>
          )}
          {list.error && (
            <p className="text-sm text-destructive">
              Failed to load identities: {(list.error as Error).message}
            </p>
          )}
          <ul data-testid="identity-list" className="flex flex-col gap-2.5 overflow-y-auto">
            <AnimatePresence initial={!reduced}>
              {list.data?.identities.map((idn, i) => (
                <motion.li
                  key={idn.id}
                  initial={reduced ? false : { opacity: 0, y: 4 }}
                  animate={{ opacity: 1, y: 0 }}
                  exit={reduced ? { opacity: 0 } : { opacity: 0, y: -4 }}
                  transition={{ ...transition, delay: reduced ? 0 : Math.min(i, 12) * LIST_STAGGER }}
                >
                  <button
                    type="button"
                    onClick={() => navigate({ to: '/identity/$id', params: { id: idn.id } })}
                    className={cn(
                      'group w-full rounded-lg border border-border bg-card p-3 text-left transition-colors',
                      'hover:bg-surface-2',
                      selectedId === idn.id &&
                        'border-primary/40 bg-surface-2 shadow-glow ring-1 ring-primary/25',
                      idn.status !== 'active' && 'opacity-70',
                    )}
                  >
                    <div className="flex items-center gap-2.5">
                      <Avatar name={idn.name} />
                      <div className="min-w-0 flex-1">
                        <div className="flex items-center gap-1.5">
                          <span className="truncate text-sm font-medium leading-snug">
                            {idn.name}
                          </span>
                          <KindPill kind={idn.kind} />
                        </div>
                        <div className="truncate font-mono text-xs text-muted-foreground">
                          {idn.id}
                        </div>
                      </div>
                      <StatusDot status={idn.status} />
                    </div>
                  </button>
                </motion.li>
              ))}
            </AnimatePresence>
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

        <section className="min-w-0">
          {selectedId ? (
            <IdentityDetail id={selectedId} />
          ) : (
            <div className="flex h-full items-center justify-center rounded-lg border border-dashed border-border bg-surface-1/40 px-6 py-12 text-center text-sm text-muted-foreground">
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
      <div className="space-y-4 rounded-lg border border-border bg-card p-4">
        <div className="flex items-center gap-3">
          <Skeleton className="h-10 w-10 rounded-full" />
          <div className="flex-1 space-y-2">
            <Skeleton className="h-5 w-1/3" />
            <Skeleton className="h-3.5 w-1/2" />
          </div>
        </div>
        <Skeleton className="h-48 w-full rounded-lg" />
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
      <header className="space-y-3 rounded-lg border border-border bg-card p-4 shadow-elevation-1">
        <div className="flex items-center gap-2 font-mono text-xs uppercase tracking-wider text-muted-foreground">
          <span>identity</span>
          <span className="text-border">/</span>
          <span className="text-foreground/70">{identity.id}</span>
        </div>
        <div className="flex flex-wrap items-center gap-3">
          <Avatar name={identity.name} size="lg" />
          <h2 className="font-display text-xl font-semibold leading-snug tracking-tight">
            {identity.name}
          </h2>
          <KindPill kind={identity.kind} />
          <StatusDot status={identity.status} />
        </div>
        {(identity.emails.length > 0 || identity.machines.length > 0) && (
          <div className="flex flex-wrap items-center gap-1.5 text-xs text-muted-foreground">
            {identity.emails.map((e) => (
              <span key={e} className="rounded-md bg-muted px-1.5 py-0.5 font-mono">
                {e}
              </span>
            ))}
            {identity.machines.map((m) => (
              <span key={m} className="rounded-md bg-muted px-1.5 py-0.5 font-mono">
                {m}
              </span>
            ))}
          </div>
        )}
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

      <section className="space-y-2.5">
        <h3 className="text-sm font-medium uppercase tracking-wide text-muted-foreground">
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

/**
 * Avatar chip matching the board card avatar style: a small colored circle
 * carrying the identity's leading initial.
 */
function Avatar({ name, size = 'sm' }: { name: string; size?: 'sm' | 'lg' }) {
  const initial = name.trim().charAt(0).toUpperCase() || '?'
  return (
    <span
      className={cn(
        'flex shrink-0 items-center justify-center rounded-full bg-primary/20 font-mono font-semibold text-primary',
        size === 'lg' ? 'h-10 w-10 text-base' : 'h-8 w-8 text-sm',
      )}
    >
      {initial}
    </span>
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
        'inline-flex shrink-0 items-center gap-1 font-mono text-[0.625rem] uppercase tracking-wider',
        active ? 'text-success' : 'text-muted-foreground',
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

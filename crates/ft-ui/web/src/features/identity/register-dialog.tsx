/**
 * Register-identity dialog. Mirrors the W2 memory create-dialog shape.
 *
 * Capability overrides are parsed from a comma-separated `key=value` field —
 * cheap to type, matches the CLI's flag-based shape, and avoids building a
 * multi-select against an unknown capability list.
 */
import { useState } from 'react'
import { useForm, type SubmitHandler } from 'react-hook-form'
import { zodResolver } from '@hookform/resolvers/zod'
import { z } from 'zod'
import { Loader2 } from 'lucide-react'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from '@/components/ui/dialog'
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
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import type { CapabilityOverrideInput } from '@/api/types/CapabilityOverrideInput'
import type { IdentityKindInput } from '@/api/types/IdentityKindInput'
import { useRegisterIdentity } from './use-identity-query'
import { Field } from '@/features/memory/forms/shared'

const KIND_OPTIONS: IdentityKindInput[] = ['human', 'bot', 'ci']

const schema = z.object({
  id: z
    .string()
    .min(1, 'Required')
    .regex(/^[a-zA-Z0-9_-]+$/, 'Use letters, digits, dashes, underscores'),
  name: z.string().min(1, 'Required'),
  email: z
    .string()
    .min(1, 'Required')
    .refine((s) => /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(s), { message: 'Must be a valid email' }),
  kind: z.enum(['human', 'bot', 'ci']),
  capabilities: z.string().optional(),
  machines: z.string().optional(),
})

type FormValues = z.infer<typeof schema>

function parseCapabilities(input?: string): CapabilityOverrideInput[] {
  if (!input) return []
  return input
    .split(',')
    .map((s) => s.trim())
    .filter(Boolean)
    .map((entry) => {
      const [key, value] = entry.split('=').map((s) => s.trim())
      return {
        key,
        value: value === undefined ? true : value.toLowerCase() === 'true',
      }
    })
    .filter((c) => c.key)
}

function parseCsv(input?: string): string[] {
  if (!input) return []
  return input.split(',').map((s) => s.trim()).filter(Boolean)
}

interface RegisterDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

export function RegisterIdentityDialog({ open, onOpenChange }: RegisterDialogProps) {
  const mutate = useRegisterIdentity()
  const form = useForm<FormValues>({
    resolver: zodResolver(schema) as never,
    defaultValues: { id: '', name: '', email: '', kind: 'human', capabilities: '', machines: '' },
  })

  const onSubmit: SubmitHandler<FormValues> = async (v) => {
    await mutate.mutateAsync({
      id: v.id,
      name: v.name,
      emails: [v.email],
      kind: v.kind,
      machines: parseCsv(v.machines),
      capabilities: parseCapabilities(v.capabilities),
    })
    form.reset()
    onOpenChange(false)
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle className="font-mono">Register identity</DialogTitle>
        </DialogHeader>
        <form id="register-identity-form" onSubmit={form.handleSubmit(onSubmit)} className="space-y-4">
          <Field label="Id" error={form.formState.errors.id?.message}>
            <Input autoFocus {...form.register('id')} placeholder="alice / bot-claude" />
          </Field>
          <Field label="Name" error={form.formState.errors.name?.message}>
            <Input {...form.register('name')} placeholder="Alice Example" />
          </Field>
          <Field label="Email" error={form.formState.errors.email?.message}>
            <Input {...form.register('email')} placeholder="alice@example.com" />
          </Field>
          <div className="grid grid-cols-2 gap-3">
            <Field label="Kind">
              <Select
                value={form.watch('kind')}
                onValueChange={(v) => form.setValue('kind', v as IdentityKindInput)}
              >
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {KIND_OPTIONS.map((k) => (
                    <SelectItem key={k} value={k}>
                      {k}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </Field>
            <Field label="Machines (csv)">
              <Input {...form.register('machines')} placeholder="host1, host2" />
            </Field>
          </div>
          <Field label="Capability overrides (key=value, …)">
            <Input
              {...form.register('capabilities')}
              placeholder="can_promote_verified=true, can_redact=false"
            />
            <p className="mt-1 text-[0.65rem] text-muted-foreground">
              Comma-separated overrides. Bare keys default to <code>true</code>.
            </p>
          </Field>
          <DialogFooter>
            <Button
              type="submit"
              form="register-identity-form"
              disabled={mutate.isPending}
              className="gap-2"
            >
              {mutate.isPending && <Loader2 className="h-4 w-4 animate-spin" />}
              Register
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}

/**
 * Offboarding is destructive: it flips the identity to `offboarded` and (with
 * `sweepClaims: true`, which the API wrapper sets unconditionally) releases
 * every claim the identity currently holds. We use an `AlertDialog` so the
 * action requires a deliberate confirmation rather than a click-outside
 * dismissal, and we surface the count of released claims after success.
 */
export function OffboardConfirmDialog({
  id,
  open,
  onOpenChange,
}: {
  id: string
  open: boolean
  onOpenChange: (b: boolean) => void
}) {
  const mutate = useOffboard(id)
  const [result, setResult] = useState<number | null>(null)
  // While the result is shown, fall back to a regular Dialog so the user can
  // dismiss the success state. The destructive confirmation flow uses
  // AlertDialog (no click-outside dismiss).
  if (result !== null) {
    return (
      <Dialog open={open} onOpenChange={onOpenChange}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle className="font-mono">Offboarded {id}</DialogTitle>
          </DialogHeader>
          <p className="text-sm text-foreground">
            <span className="font-mono text-primary">{result}</span> claim
            {result === 1 ? '' : 's'} released.
          </p>
          <DialogFooter>
            <Button variant="outline" onClick={() => onOpenChange(false)}>
              Close
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    )
  }
  return (
    <AlertDialog open={open} onOpenChange={onOpenChange}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle className="font-mono">Offboard {id}?</AlertDialogTitle>
          <AlertDialogDescription>
            This marks the identity as offboarded and releases every claim they
            currently hold (sweep). The transition is recorded in the history
            chain and cannot be reversed without re-registering the identity.
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel onClick={() => onOpenChange(false)}>Cancel</AlertDialogCancel>
          <AlertDialogAction
            disabled={mutate.isPending}
            onClick={async (e) => {
              e.preventDefault()
              const out = await mutate.mutateAsync()
              setResult(out.claimsReleased)
            }}
            className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
          >
            {mutate.isPending && <Loader2 className="mr-1 h-4 w-4 animate-spin" />}
            Offboard
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}

// Importing here (rather than at the top) keeps the file's React import order
// stable while sidestepping a hoist-order import cycle in tests.
import { useOffboardIdentity as useOffboard } from './use-identity-query'

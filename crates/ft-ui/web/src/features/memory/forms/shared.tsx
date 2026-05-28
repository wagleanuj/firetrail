/**
 * Tiny shared bits used by every per-kind memory create form.
 *
 * Kept here (rather than re-exported from the tickets surface) so the
 * memory feature doesn't pull in `features/tickets/*` for its layout
 * primitives — fewer cross-feature edges, easier to evolve.
 */
import type { ReactNode } from 'react'
import { Loader2 } from 'lucide-react'
import { DialogFooter } from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { RISK_CLASSES } from '../types'

export function Field({
  label,
  error,
  children,
}: {
  label: string
  error?: string
  children: ReactNode
}) {
  return (
    <div className="space-y-1.5">
      <Label>{label}</Label>
      {children}
      {error && <p className="text-xs text-destructive">{error}</p>}
    </div>
  )
}

export function FormShell({
  children,
  submitting,
  formId,
}: {
  children: ReactNode
  submitting: boolean
  formId: string
}) {
  return (
    <div className="space-y-4">
      {children}
      <DialogFooter>
        <Button type="submit" form={formId} disabled={submitting} className="gap-2">
          {submitting && <Loader2 className="h-4 w-4 animate-spin" />}
          Create
        </Button>
      </DialogFooter>
    </div>
  )
}

export function RiskClassSelect({
  value,
  onChange,
}: {
  value?: string
  onChange: (v: string | undefined) => void
}) {
  return (
    <Select value={value} onValueChange={(v) => onChange(v || undefined)}>
      <SelectTrigger>
        <SelectValue placeholder="Risk class" />
      </SelectTrigger>
      <SelectContent>
        {RISK_CLASSES.map((r) => (
          <SelectItem key={r} value={r}>
            {r}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  )
}

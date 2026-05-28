/**
 * Capability override editor.
 *
 * Surfaces the four well-known capabilities as tri-state controls:
 *
 *   - "default" — clears the override and uses the kind default
 *   - "allow"   — explicit `true`
 *   - "deny"    — explicit `false`
 *
 * The current draft is local until "Save" is clicked. The mutation
 * invalidates both the capabilities matrix and the identity show query so
 * the matrix view and the override pills refresh together.
 */
import { useMemo, useState } from 'react'
import { Loader2 } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Skeleton } from '@/components/ui/skeleton'
import type { CapabilityOverride } from '@/api/types/CapabilityOverride'
import { useIdentity, useUpdateCapabilities } from './use-identity-query'

const WELL_KNOWN: ReadonlyArray<{ key: string; label: string }> = [
  { key: 'can_promote_verified', label: 'Promote verified' },
  { key: 'can_close_high_risk', label: 'Close high-risk' },
  { key: 'can_force_push', label: 'Force push' },
  { key: 'can_redact', label: 'Redact records' },
]

type TriState = 'default' | 'allow' | 'deny'

function overrideFor(
  overrides: CapabilityOverride[],
  key: string,
): TriState {
  const found = overrides.find((o) => o.key === key)
  if (!found) return 'default'
  return found.value ? 'allow' : 'deny'
}

function toPatchValue(state: TriState): boolean | null {
  if (state === 'default') return null
  return state === 'allow'
}

export function CapabilityEditor({ identity }: { identity: string }) {
  const { data, isLoading } = useIdentity(identity)
  const update = useUpdateCapabilities(identity)

  const initial = useMemo<Record<string, TriState>>(() => {
    if (!data) return {}
    return Object.fromEntries(
      WELL_KNOWN.map((c) => [c.key, overrideFor(data.identity.capabilities, c.key)]),
    )
  }, [data])

  const [draft, setDraft] = useState<Record<string, TriState> | null>(null)
  const effective = draft ?? initial
  const dirty = Object.keys(effective).some((k) => effective[k] !== initial[k])

  if (isLoading || !data) return <Skeleton className="h-32 w-full" />

  return (
    <div className="space-y-3 rounded-md border border-border/70 bg-background/60 p-4">
      <header className="flex items-center justify-between">
        <h3 className="font-mono text-xs uppercase tracking-wider text-muted-foreground">
          Edit capability overrides
        </h3>
        <div className="flex items-center gap-2">
          {dirty && (
            <Button
              size="sm"
              variant="ghost"
              onClick={() => setDraft(null)}
              disabled={update.isPending}
            >
              Discard
            </Button>
          )}
          <Button
            size="sm"
            disabled={!dirty || update.isPending}
            onClick={() => {
              if (!draft) return
              const patches = WELL_KNOWN.filter(
                (c) => draft[c.key] !== initial[c.key],
              ).map((c) => ({
                key: c.key,
                value: toPatchValue(draft[c.key]),
              }))
              update.mutate(patches, {
                onSuccess: () => setDraft(null),
              })
            }}
            className="gap-2"
          >
            {update.isPending && <Loader2 className="h-3 w-3 animate-spin" />}
            Save
          </Button>
        </div>
      </header>
      <div
        data-testid="capability-editor"
        className="grid grid-cols-1 gap-3 sm:grid-cols-2"
      >
        {WELL_KNOWN.map(({ key, label }) => {
          const state = effective[key] ?? 'default'
          return (
            <div key={key} className="space-y-1">
              <Label className="text-xs">{label}</Label>
              <Select
                value={state}
                onValueChange={(v) =>
                  setDraft({ ...effective, [key]: v as TriState })
                }
              >
                <SelectTrigger
                  data-testid={`cap-trigger-${key}`}
                  className="h-8 text-xs"
                >
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="default">default (use kind)</SelectItem>
                  <SelectItem value="allow">allow</SelectItem>
                  <SelectItem value="deny">deny</SelectItem>
                </SelectContent>
              </Select>
              <p className="font-mono text-[0.625rem] uppercase tracking-wider text-muted-foreground">
                {key}
              </p>
            </div>
          )
        })}
      </div>
    </div>
  )
}

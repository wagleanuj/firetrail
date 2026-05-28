/**
 * Memory mutation hooks. Each one:
 *   - performs the network round-trip via `./api`
 *   - applies an optimistic cache patch where it makes sense
 *   - falls back to invalidate-on-success for low-frequency ops
 *   - surfaces typed errors via `toastApiError`
 *
 * Cloned from `features/tickets/use-ticket-mutations.ts` — keep the shape
 * (`onMutate`/`onError`/`onSuccess`) symmetric across surfaces.
 */
import { useMutation, useQueryClient, type UseMutationResult } from '@tanstack/react-query'
import { toast } from 'sonner'
import type { MemoryRowOut } from '@/api/types/MemoryRowOut'
import type { SalvageInput } from '@/api/types/SalvageInput'
import type { SalvageOutput } from '@/api/types/SalvageOutput'
import { toastApiError } from '@/api/error'
import {
  createMemory,
  postSalvage,
  type CreateMemoryResponse,
  type MemoryListResponse,
} from './api'
import type { MemoryCreateBody } from './types'

/** POST /api/memory — discriminated create. */
export function useCreateMemory(): UseMutationResult<
  CreateMemoryResponse,
  unknown,
  MemoryCreateBody
> {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (body) => createMemory(body),
    onSuccess: (out) => {
      const env = out.record.envelope
      const row: MemoryRowOut = {
        id: env.id,
        kind: env.kind,
        title: env.title,
        trust: null,
        riskClass: null,
        stale: false,
      }
      qc.setQueriesData<MemoryListResponse>({ queryKey: ['memory-list'] }, (prev) => {
        if (!prev) return prev
        if (prev.rows.some((r) => r.id === row.id)) return prev
        return { rows: [row, ...prev.rows] }
      })
      toast.success(`Created ${env.id.slice(0, 14)}`)
      qc.invalidateQueries({ queryKey: ['memory-list'] })
    },
    onError: (err) => toastApiError(err),
  })
}

export interface SalvagePlanVars {
  base?: string
  branch?: string | null
}

/** POST /api/memory/salvage { dryRun: true } — discovery step. */
export function useSalvageDryRun(): UseMutationResult<
  SalvageOutput,
  unknown,
  SalvagePlanVars | void
> {
  return useMutation({
    mutationFn: (vars) =>
      postSalvage({
        base: vars?.base,
        branch: vars?.branch ?? null,
        dryRun: true,
      }),
    onError: (err) => toastApiError(err, 'Salvage scan failed'),
  })
}

export interface SalvageApplyVars {
  base?: string
  branch?: string | null
  selected: string[]
}

/** POST /api/memory/salvage { dryRun: false, selected } — apply step. */
export function useSalvageApply(): UseMutationResult<
  SalvageOutput,
  unknown,
  SalvageApplyVars
> {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (vars) => {
      const body: Partial<Omit<SalvageInput, 'requestId'>> = {
        base: vars.base ?? 'main',
        branch: vars.branch ?? null,
        dryRun: false,
        selected: vars.selected,
      }
      return postSalvage(body)
    },
    onSuccess: (out) => {
      const accepted = out.entries.filter((e) => e.action === 'salvaged').length
      const skipped = out.entries.filter((e) => e.action === 'skipped').length
      toast.success(`Salvage applied: ${accepted} accepted, ${skipped} skipped`)
      qc.invalidateQueries({ queryKey: ['memory-list'] })
    },
    onError: (err) => toastApiError(err, 'Salvage apply failed'),
  })
}

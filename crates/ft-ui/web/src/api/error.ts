/**
 * Re-export ApiError + a sonner-friendly toast helper. ApiError itself lives in
 * `@/api/client` (where the fetch wrapper constructs it), but consumers should
 * import from here so we have a single place to evolve the error taxonomy.
 *
 * The error → string mapping lives in `@/lib/errors::humanizeError` so it can
 * be reused by error boundaries and inline banners. This module is the
 * sonner-aware wrapper that fires the toast.
 */
import { toast } from 'sonner'
import { AlertCircle } from 'lucide-react'
import * as React from 'react'
import { ApiError } from '@/api/client'
import { humanizeError } from '@/lib/errors'

export { ApiError } from '@/api/client'

/**
 * Map an unknown thrown value (almost always `ApiError`) to a user-facing
 * sonner toast. Returns the resolved title so callers can also surface it
 * inline if they like.
 */
export function toastApiError(err: unknown, fallback = 'Something went wrong'): string {
  const human = humanizeError(err, fallback)
  toast.error(human.title, {
    description: human.description,
    icon: React.createElement(AlertCircle, { className: 'h-4 w-4 text-destructive' }),
  })
  return human.title
}

// Hint to keep the original ApiError export reachable for callers that want
// the typed narrow rather than going through `humanizeError`.
export type { ApiError as ApiErrorType } from '@/api/client'
void ApiError

// --- Iconised toast helpers -------------------------------------------------
// Centralise sonner.success / sonner.info / sonner.warning so every variant
// renders the same lucide icon. Existing call-sites that use bare
// `toast.success(...)` still work; the helpers here are opt-in sugar.
import { CheckCircle, AlertTriangle, Info } from 'lucide-react'

export function toastSuccess(title: string, description?: string) {
  toast.success(title, {
    description,
    icon: React.createElement(CheckCircle, { className: 'h-4 w-4 text-primary' }),
  })
}

export function toastWarning(title: string, description?: string) {
  toast.warning(title, {
    description,
    icon: React.createElement(AlertTriangle, { className: 'h-4 w-4 text-amber-400' }),
  })
}

export function toastInfo(title: string, description?: string) {
  toast.info(title, {
    description,
    icon: React.createElement(Info, { className: 'h-4 w-4 text-primary' }),
  })
}

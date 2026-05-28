/**
 * Re-export ApiError + a sonner-friendly toast helper. ApiError itself lives in
 * `@/api/client` (where the fetch wrapper constructs it), but consumers should
 * import from here so we have a single place to evolve the error taxonomy.
 */
import { toast } from 'sonner'
import { ApiError } from '@/api/client'

export { ApiError } from '@/api/client'

/**
 * Map an unknown thrown value (almost always `ApiError`) to a user-facing
 * sonner toast. Returns the resolved title so callers can also surface it
 * inline if they like.
 */
export function toastApiError(err: unknown, fallback = 'Something went wrong'): string {
  if (err instanceof ApiError) {
    const title = titleForKind(err.kind, err.message)
    toast.error(title, { description: err.field ? `field: ${err.field}` : undefined })
    return title
  }
  const msg = err instanceof Error ? err.message : fallback
  toast.error(fallback, { description: msg })
  return fallback
}

function titleForKind(kind: string, message: string): string {
  switch (kind) {
    case 'not_found':
      return 'Ticket not found'
    case 'validation':
      return message || 'Validation failed'
    case 'conflict':
      return message || 'Conflict — refresh and try again'
    case 'permission_denied':
      return message || 'Permission denied'
    case 'internal':
      return 'Something went wrong — check the server logs'
    default:
      return message || `Error: ${kind}`
  }
}

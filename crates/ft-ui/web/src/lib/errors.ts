/**
 * Friendlier error rendering helpers.
 *
 * `humanizeError` maps an unknown thrown value (almost always `ApiError` from
 * `@/api/client`) into a `{ title, description? }` pair that's safe to show in
 * a toast or banner. The base mapping lives in `@/api/error::toastApiError`,
 * but this helper returns a plain object so callers can use it inline (e.g.
 * inside an Alert component) rather than firing a toast.
 */
import { ApiError } from '@/api/client'

export interface HumanError {
  title: string
  description?: string
}

export function humanizeError(err: unknown, fallback = 'Something went wrong'): HumanError {
  if (err instanceof ApiError) {
    return {
      title: titleForKind(err.kind, err.message),
      description: descriptionForKind(err),
    }
  }
  if (err instanceof Error) {
    return { title: fallback, description: err.message }
  }
  return { title: fallback }
}

function titleForKind(kind: string, message: string): string {
  switch (kind) {
    case 'not_found':
      return 'Not found'
    case 'validation':
      return message || 'Validation failed'
    case 'conflict':
      return 'Conflict — refresh and try again'
    case 'permission_denied':
      return 'Permission denied'
    case 'internal':
      return 'Server error'
    case 'rate_limited':
      return 'Too many requests'
    default:
      return message || `Error: ${kind}`
  }
}

function descriptionForKind(err: ApiError): string | undefined {
  if (err.field) return `field: ${err.field}`
  switch (err.kind) {
    case 'internal':
      return 'Check the server logs for the request id.'
    case 'conflict':
      return err.message || 'The record state changed since you loaded it.'
    case 'permission_denied':
      return err.message || 'Your identity does not have the required capability.'
    case 'not_found':
      return err.message || undefined
    default:
      return err.message || undefined
  }
}

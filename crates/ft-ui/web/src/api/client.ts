import { newRequestId, trackRequestId } from '@/lib/request-id'

/** Shape returned by ft-ui's JSON error envelope. */
export interface ApiErrorBody {
  error: {
    kind: string
    message: string
    field?: string
  }
}

export class ApiError extends Error {
  readonly status: number
  readonly kind: string
  readonly field?: string

  constructor(status: number, body: ApiErrorBody) {
    super(body.error.message)
    this.status = status
    this.kind = body.error.kind
    this.field = body.error.field
    this.name = 'ApiError'
  }
}

export interface RequestOptions extends Omit<RequestInit, 'body'> {
  body?: unknown
  /** Set to false to skip generating a request id (e.g. for pure reads). */
  withRequestId?: boolean
}

/**
 * Thin fetch wrapper used by every hook in @/api/hooks.
 *
 * - Always sends `credentials: 'include'` so a future cookie-based identity
 *   session works without per-call wiring.
 * - For non-GET requests, mints an `X-Firetrail-Request-Id` and registers it
 *   with the local request-id store so `useEvents` can filter the echo.
 * - Parses `application/json` responses; throws `ApiError` on non-2xx.
 */
export async function apiFetch<T = unknown>(
  path: string,
  options: RequestOptions = {},
): Promise<T> {
  const { body, headers, withRequestId, ...rest } = options
  const method = rest.method ?? (body !== undefined ? 'POST' : 'GET')
  const finalHeaders = new Headers(headers)
  finalHeaders.set('Accept', 'application/json')

  if (body !== undefined && !finalHeaders.has('Content-Type')) {
    finalHeaders.set('Content-Type', 'application/json')
  }

  const wantId = withRequestId ?? method !== 'GET'
  if (wantId) {
    const id = newRequestId()
    trackRequestId(id)
    finalHeaders.set('X-Firetrail-Request-Id', id)
  }

  const response = await fetch(path, {
    ...rest,
    method,
    headers: finalHeaders,
    credentials: 'include',
    body: body !== undefined ? JSON.stringify(body) : undefined,
  })

  if (response.status === 204) {
    return undefined as T
  }

  const contentType = response.headers.get('content-type') ?? ''
  const isJson = contentType.includes('application/json')
  const payload = isJson ? ((await response.json()) as unknown) : ((await response.text()) as unknown)

  if (!response.ok) {
    if (isJson && payload && typeof payload === 'object' && 'error' in payload) {
      throw new ApiError(response.status, payload as ApiErrorBody)
    }
    throw new ApiError(response.status, {
      error: { kind: 'http_error', message: `HTTP ${response.status}` },
    })
  }

  return payload as T
}

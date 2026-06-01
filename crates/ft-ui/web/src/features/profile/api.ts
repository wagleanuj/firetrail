/**
 * Repo-profile endpoints (RepoProfile epic). Thin `apiFetch` wrappers — the
 * caching/optimism layer lives in `use-profile-query.ts`.
 *
 *   GET    /api/profile                  → current RepoProfile (404 if none)
 *   PUT    /api/profile                  → partial update
 *   POST   /api/profile/components       → add { name, path, summary? }
 *   DELETE /api/profile/components/:name → remove one
 *
 * Confirmation (Draft → Reviewed → Verified) goes through the existing
 * `/api/trust/*` routes — see `@/features/trust/api`.
 */
import { apiFetch, ApiError } from '@/api/client'
import type { ProfileView } from '@/api/types/ProfileView'
import type { ScopeListOutput } from '@/api/types/ScopeListOutput'
import type { ScopeSummary } from '@/api/types/ScopeSummary'

/**
 * Which profile to fetch: the base singleton, or a per-scope delta — and, for a
 * scope, whether to resolve it against the base (the merged view).
 */
export interface ProfileSelector {
  /** Scope id; `null`/absent selects the base profile. */
  scope?: string | null
  /** When true (and `scope` set), return the base-merged view. */
  resolved?: boolean
}

/** Build the `/api/profile` query string for a selector. */
function selectorQuery({ scope, resolved }: ProfileSelector): string {
  const params = new URLSearchParams()
  if (scope) {
    params.set('scope', scope)
    if (resolved) params.set('resolved', '1')
  }
  const qs = params.toString()
  return qs ? `?${qs}` : ''
}

/** A single-field partial update. `null` clears the field; omitting leaves it. */
export interface ProfilePatch {
  validateCommand?: string | null
  testCommand?: string | null
  buildCommand?: string | null
  lintCommand?: string | null
  languages?: string[]
  packageManagers?: string[]
  runtime?: string | null
  notes?: string | null
}

/**
 * GET /api/profile[?scope=&resolved=1] — resolves to `null` when no profile (or
 * scope delta) exists yet (404). Base when `selector` is omitted.
 */
export async function fetchProfile(selector: ProfileSelector = {}): Promise<ProfileView | null> {
  try {
    return await apiFetch<ProfileView>(`/api/profile${selectorQuery(selector)}`, {
      withRequestId: false,
    })
  } catch (err) {
    if (err instanceof ApiError && err.status === 404) return null
    throw err
  }
}

/** GET /api/scope — every scope declared in `.firetrail/scopes.yaml`. */
export async function fetchScopes(): Promise<ScopeSummary[]> {
  try {
    const out = await apiFetch<ScopeListOutput>('/api/scope', { withRequestId: false })
    return out.scopes
  } catch (err) {
    // No scopes.yaml (standalone repo) → empty list, not an error.
    if (err instanceof ApiError && err.status === 404) return []
    throw err
  }
}

/**
 * PUT /api/profile[?scope=] — partial update; creates the record if absent.
 * `resolved` is ignored on a write.
 */
export function updateProfile(
  patch: ProfilePatch,
  selector: ProfileSelector = {},
): Promise<ProfileView> {
  return apiFetch<ProfileView>(`/api/profile${selectorQuery({ scope: selector.scope })}`, {
    method: 'PUT',
    body: patch,
  })
}

/** POST /api/profile/components — add (or replace by name) a component. */
export function addComponent(
  name: string,
  path: string,
  summary?: string,
): Promise<ProfileView> {
  return apiFetch<ProfileView>('/api/profile/components', {
    method: 'POST',
    body: { name, path, summary: summary ?? null },
  })
}

/** DELETE /api/profile/components/:name — remove one component. */
export function removeComponent(name: string): Promise<ProfileView> {
  return apiFetch<ProfileView>(`/api/profile/components/${encodeURIComponent(name)}`, {
    method: 'DELETE',
  })
}

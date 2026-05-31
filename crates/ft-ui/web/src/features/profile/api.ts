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

/** GET /api/profile — resolves to `null` when no profile exists yet (404). */
export async function fetchProfile(): Promise<ProfileView | null> {
  try {
    return await apiFetch<ProfileView>('/api/profile', { withRequestId: false })
  } catch (err) {
    if (err instanceof ApiError && err.status === 404) return null
    throw err
  }
}

/** PUT /api/profile — partial update; creates the profile if absent. */
export function updateProfile(patch: ProfilePatch): Promise<ProfileView> {
  return apiFetch<ProfileView>('/api/profile', { method: 'PUT', body: patch })
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

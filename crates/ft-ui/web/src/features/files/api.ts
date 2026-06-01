/**
 * Thin wrapper around `apiFetch` for the file-autocomplete surface.
 *
 * `GET /api/files` returns a flat list of repo-relative, forward-slash paths
 * (optionally restricted to directories) for typeahead in path inputs.
 */
import { apiFetch } from '@/api/client'
import type { FileListView } from '@/api/types/FileListView'

/** GET /api/files?prefix=&dirs=&limit= */
export function fetchFiles(
  prefix: string,
  dirs: boolean,
  limit = 50,
): Promise<FileListView> {
  const q = new URLSearchParams({
    prefix,
    dirs: String(dirs),
    limit: String(limit),
  })
  return apiFetch<FileListView>(`/api/files?${q.toString()}`)
}

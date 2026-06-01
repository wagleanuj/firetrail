/**
 * TanStack Query hook for the file-autocomplete surface (`GET /api/files`).
 *
 * Read-only: feeds path typeahead. Keyed on the (prefix, dirs) pair so each
 * distinct prefix gets its own cache entry; callers debounce the prefix
 * upstream so the cache key stabilises between keystrokes.
 */
import { useQuery, type UseQueryResult } from '@tanstack/react-query'
import type { FileListView } from '@/api/types/FileListView'
import { fetchFiles } from './api'

export const filesKey = (prefix: string, dirs: boolean) =>
  ['files', { prefix, dirs }] as const

export function useFiles(
  prefix: string,
  dirs: boolean,
): UseQueryResult<FileListView> {
  return useQuery({
    queryKey: filesKey(prefix, dirs),
    queryFn: () => fetchFiles(prefix, dirs),
    staleTime: 10_000,
  })
}

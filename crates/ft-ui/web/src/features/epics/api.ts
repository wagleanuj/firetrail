/**
 * Thin per-endpoint wrapper around `apiFetch` for the epics surface.
 */
import { apiFetch } from '@/api/client'
import type { EpicsOutput } from '@/api/types/EpicsOutput'

export function fetchEpics(): Promise<EpicsOutput> {
  return apiFetch<EpicsOutput>('/api/epics')
}

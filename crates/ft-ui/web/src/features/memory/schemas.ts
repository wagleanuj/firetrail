/**
 * Zod schemas — one per memory kind — used by the create dialog.
 *
 * Each schema mirrors the relevant `Create*Input` in `@/api/types/` but
 * collapses optional null-able fields into ergonomic strings, and adds the
 * non-negotiable "title/summary is required" rule.
 */
import { z } from 'zod'
import { RISK_CLASSES, SEVERITIES } from './types'

const commaList = z
  .string()
  .optional()
  .transform((s) =>
    (s ?? '')
      .split(',')
      .map((x) => x.trim())
      .filter(Boolean),
  )

export const incidentSchema = z.object({
  summary: z.string().min(1, 'Summary is required'),
  severity: z.enum(SEVERITIES).optional(),
  services: commaList,
  riskClass: z.enum(RISK_CLASSES).optional(),
  scope: z.string().optional().default(''),
  rootCause: z.string().optional().default(''),
  resolvedAt: z.string().optional().default(''),
  findings: commaList,
  runbooksInvoked: commaList,
})

export const findingSchema = z.object({
  summary: z.string().min(1, 'Summary is required'),
  incident: z.string().optional().default(''),
  details: z.string().optional().default(''),
  affected: commaList,
  riskClass: z.enum(RISK_CLASSES).optional(),
  scope: z.string().optional().default(''),
})

export const runbookSchema = z.object({
  title: z.string().min(1, 'Title is required'),
  summary: z.string().min(1, 'Summary is required'),
  appliesTo: commaList,
  riskClass: z.enum(RISK_CLASSES).optional(),
  scope: z.string().optional().default(''),
})

export const DECISION_STATUSES = [
  'proposed',
  'accepted',
  'superseded',
  'deprecated',
] as const

export const decisionSchema = z.object({
  title: z.string().min(1, 'Title is required'),
  context: z.string().min(1, 'Context is required'),
  decision: z.string().min(1, 'Decision is required'),
  consequences: z.string().optional().default(''),
  riskClass: z.enum(RISK_CLASSES).optional(),
  scope: z.string().optional().default(''),
  alternatives: commaList,
  status: z.enum(DECISION_STATUSES).optional(),
})

export const gotchaSchema = z.object({
  summary: z.string().min(1, 'Summary is required'),
  details: z.string().optional().default(''),
  affected: commaList,
  riskClass: z.enum(RISK_CLASSES).optional(),
  scope: z.string().optional().default(''),
})

export const memorySchema = z.object({
  title: z.string().min(1, 'Title is required'),
  body: z.string().min(1, 'Body is required'),
  tags: commaList,
  riskClass: z.enum(RISK_CLASSES).optional(),
  scope: z.string().optional().default(''),
})

/** Test-friendly index, exported for vitest in `schemas.test.ts`. */
export const MEMORY_SCHEMAS = {
  incident: incidentSchema,
  finding: findingSchema,
  runbook: runbookSchema,
  decision: decisionSchema,
  gotcha: gotchaSchema,
  memory: memorySchema,
} as const

export type IncidentValues = z.input<typeof incidentSchema>
export type FindingValues = z.input<typeof findingSchema>
export type RunbookValues = z.input<typeof runbookSchema>
export type DecisionValues = z.input<typeof decisionSchema>
export type GotchaValues = z.input<typeof gotchaSchema>
export type MemoryValues = z.input<typeof memorySchema>

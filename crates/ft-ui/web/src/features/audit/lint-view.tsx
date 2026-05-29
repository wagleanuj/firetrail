/**
 * Lint view. Run-on-demand POST that caches the last result. Findings table
 * supports filtering by severity and rule, and the record-id column links
 * to the correct detail page based on its prefix.
 */
import { useMemo, useState } from 'react'
import { useMutation } from '@tanstack/react-query'
import { Link } from '@tanstack/react-router'
import { Loader2, AlertTriangle, AlertCircle, ChevronDown, ChevronRight, Play } from 'lucide-react'
import type { LintOutput } from '@/api/types/LintOutput'
import type { LintSeverity } from '@/api/types/LintSeverity'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { PageHeader } from '@/components/page-header'
import { toastApiError } from '@/api/error'
import { cn } from '@/lib/utils'
import { postLint } from './api'

interface LintViewProps {
  /** Optional initial result so the dashboard can pass through a cached run. */
  initial?: LintOutput | null
}

export function LintView({ initial }: LintViewProps) {
  const [data, setData] = useState<LintOutput | null>(initial ?? null)
  const [severity, setSeverity] = useState<LintSeverity | undefined>()
  const [ruleFilter, setRuleFilter] = useState('')
  const [expanded, setExpanded] = useState<Set<number>>(new Set())
  const [lastRun, setLastRun] = useState<string | null>(null)

  const mut = useMutation({
    mutationFn: () => postLint(true),
    onSuccess: (out) => {
      setData(out)
      setLastRun(new Date().toLocaleString())
    },
    onError: (err) => toastApiError(err, 'Lint failed'),
  })

  const findings = useMemo(() => {
    if (!data) return []
    return data.findings.filter((f) => {
      if (severity && f.severity !== severity) return false
      if (ruleFilter && !f.rule.toLowerCase().includes(ruleFilter.toLowerCase())) return false
      return true
    })
  }, [data, severity, ruleFilter])

  function toggle(i: number) {
    setExpanded((prev) => {
      const next = new Set(prev)
      if (next.has(i)) next.delete(i)
      else next.add(i)
      return next
    })
  }

  return (
    <div className="space-y-6">
      <PageHeader
        title="Lint"
        subtitle={
          data ? (
            <span data-testid="lint-summary">
              Scanned {data.scanned} · {data.errors} errors · {data.warnings} warnings
              {lastRun && ` · last run ${lastRun}`}
            </span>
          ) : (
            'Scan every record for rule violations and surface suggested fixes.'
          )
        }
        actions={
          <Button
            size="sm"
            onClick={() => mut.mutate()}
            disabled={mut.isPending}
            className="gap-2"
            data-testid="lint-run"
          >
            {mut.isPending ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : <Play className="h-3.5 w-3.5" />}
            {data ? 'Re-run' : 'Run lint'}
          </Button>
        }
      />

      {data && (
        <>
          <div className="flex flex-wrap items-end gap-3 rounded-[var(--radius)] border border-border bg-surface-2 p-3">
            <div className="space-y-1.5">
              <Label className="text-xs">Severity</Label>
              <Select
                value={severity ?? '__any__'}
                onValueChange={(v) => setSeverity(v === '__any__' ? undefined : (v as LintSeverity))}
              >
                <SelectTrigger className="w-32">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="__any__">Any</SelectItem>
                  <SelectItem value="error">error</SelectItem>
                  <SelectItem value="warning">warning</SelectItem>
                </SelectContent>
              </Select>
            </div>
            <div className="flex-1 space-y-1.5">
              <Label className="text-xs">Rule</Label>
              <Input value={ruleFilter} onChange={(e) => setRuleFilter(e.target.value)} placeholder="ac_cap_exceeded" />
            </div>
          </div>

          {findings.length === 0 ? (
            <p className="rounded-[var(--radius)] border border-dashed border-border px-3 py-8 text-center text-sm text-muted-foreground">
              No findings match the filters.
            </p>
          ) : (
            <Table data-testid="lint-findings">
              <TableHeader>
                <TableRow>
                  <TableHead className="w-24">Severity</TableHead>
                  <TableHead className="w-48">Rule</TableHead>
                  <TableHead>Record</TableHead>
                  <TableHead>Message</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {findings.map((f, i) => {
                  const isOpen = expanded.has(i)
                  return (
                    <>
                      <TableRow key={`${f.rule}-${f.recordId}-${i}`}>
                        <TableCell>
                          <SeverityPill severity={f.severity} />
                        </TableCell>
                        <TableCell>
                          <code className="font-mono text-xs">{f.rule}</code>
                        </TableCell>
                        <TableCell>
                          <RecordLink id={f.recordId} />
                        </TableCell>
                        <TableCell>
                          <div className="flex items-start gap-1">
                            {f.suggestedFix && (
                              <button
                                type="button"
                                onClick={() => toggle(i)}
                                className="mt-0.5 text-muted-foreground hover:text-primary"
                                aria-label={isOpen ? 'Collapse fix' : 'Expand fix'}
                              >
                                {isOpen ? <ChevronDown className="h-3.5 w-3.5" /> : <ChevronRight className="h-3.5 w-3.5" />}
                              </button>
                            )}
                            <div className="flex-1 text-sm">{f.message}</div>
                          </div>
                        </TableCell>
                      </TableRow>
                      {isOpen && f.suggestedFix && (
                        <TableRow key={`${i}-fix`}>
                          <TableCell colSpan={4} className="bg-surface-2">
                            <div className="font-mono text-xs">
                              <span className="text-muted-foreground">Suggested fix: </span>
                              {f.suggestedFix}
                            </div>
                          </TableCell>
                        </TableRow>
                      )}
                    </>
                  )
                })}
              </TableBody>
            </Table>
          )}
        </>
      )}
    </div>
  )
}

function SeverityPill({ severity }: { severity: LintSeverity }) {
  const Icon = severity === 'error' ? AlertCircle : AlertTriangle
  return (
    <span
      className={cn(
        'inline-flex items-center gap-1 rounded px-1.5 py-0.5 font-mono text-[0.625rem] uppercase tracking-wider',
        severity === 'error'
          ? 'bg-danger/15 text-danger'
          : 'bg-warning/15 text-warning',
      )}
    >
      <Icon className="h-3 w-3" />
      {severity}
    </span>
  )
}

/**
 * Pick the right detail route for a record id based on the prefix the backend
 * encodes (e.g. `task:…` / `memory:…`). Falls back to the memory page for
 * unknown shapes.
 */
function RecordLink({ id }: { id: string }) {
  const ticketKinds = ['epic', 'task', 'subtask', 'bug']
  const isTicket = ticketKinds.some((k) => id.startsWith(`${k}:`))
  const to = isTicket ? '/tickets/$id' : '/memory/$id'
  return (
    <Link to={to} params={{ id }} className="font-mono text-xs text-primary hover:underline">
      {id}
    </Link>
  )
}

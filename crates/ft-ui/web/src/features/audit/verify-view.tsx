/**
 * Verify view. POST returns per-record verdicts; we surface a summary card
 * and a table of failures (and a collapsed "passed" section).
 */
import { useState } from 'react'
import { useMutation } from '@tanstack/react-query'
import { Play, Loader2, CheckCircle2, XCircle } from 'lucide-react'
import type { VerifyOutput } from '@/api/types/VerifyOutput'
import { Button } from '@/components/ui/button'
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
import { postVerify } from './api'

export function VerifyView() {
  const [data, setData] = useState<VerifyOutput | null>(null)
  const mut = useMutation({
    mutationFn: postVerify,
    onSuccess: (out) => setData(out),
    onError: (err) => toastApiError(err, 'Verify failed'),
  })
  const failures = data?.results.filter((r) => !r.ok) ?? []
  return (
    <div className="space-y-6">
      <PageHeader
        title="Verify"
        subtitle={
          data ? (
            <span data-testid="verify-summary">
              {data.total} records · {data.failures} failure{data.failures === 1 ? '' : 's'}
            </span>
          ) : (
            'Walk each record’s history chain and confirm signatures hold.'
          )
        }
        actions={
          <Button size="sm" onClick={() => mut.mutate()} disabled={mut.isPending} className="gap-2">
            {mut.isPending ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : <Play className="h-3.5 w-3.5" />}
            {data ? 'Re-run' : 'Run verify'}
          </Button>
        }
      />
      {data && (
        <>
          <div
            className={cn(
              'rounded-[var(--radius)] border px-4 py-3 text-sm',
              data.failures === 0
                ? 'border-success/30 bg-success/10 text-success'
                : 'border-danger/30 bg-danger/10 text-danger',
            )}
          >
            {data.failures === 0 ? (
              <span className="inline-flex items-center gap-2">
                <CheckCircle2 className="h-4 w-4" />
                All {data.total} record chains verified.
              </span>
            ) : (
              <span className="inline-flex items-center gap-2">
                <XCircle className="h-4 w-4" />
                {data.failures} of {data.total} record chains failed.
              </span>
            )}
          </div>
          {failures.length > 0 && (
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Record</TableHead>
                  <TableHead>Reason</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {failures.map((r) => (
                  <TableRow key={r.id}>
                    <TableCell>
                      <code className="font-mono text-xs">{r.id}</code>
                    </TableCell>
                    <TableCell className="text-sm text-danger">{r.reason}</TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          )}
        </>
      )}
    </div>
  )
}

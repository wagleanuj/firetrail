/**
 * Inline trust-state action panel embedded in record detail pages.
 *
 * Renders only the transitions valid from the current trust state. Each
 * action either fires immediately (review when no evidence required) or
 * opens a small confirmation dialog asking for the required inputs.
 *
 * Trust transitions are sensitive — confirmation toast on success, with a
 * dedicated message on 409 conflicts pointing at refresh-and-retry.
 */
import { useEffect, useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { toast } from 'sonner'
import {
  AlertTriangle,
  CheckCircle2,
  Archive,
  Replace,
  Eraser,
  GitMerge,
  ShieldCheck,
  Loader2,
} from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import { Label } from '@/components/ui/label'
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { ApiError, toastApiError } from '@/api/error'
import type { EvidenceKindInput } from '@/api/types/EvidenceKindInput'
import type { RecordWire } from '@/api/wire/record'
import { useEvents } from '@/api/hooks/useEvents'
import type { Event as AppEvent } from '@/api/types/Event'
import {
  postArchive,
  postDeprecate,
  postMerge,
  postPromote,
  postRedact,
  postReview,
  postSupersede,
} from './api'
import {
  isHighStakes,
  OP_LABELS,
  validOps,
  type TrustOp,
} from './state-machine'
import { memoryShowKey, useMemoryQuery } from '@/features/memory/use-memory-query'

const ICON: Record<TrustOp, React.ComponentType<{ className?: string }>> = {
  review: ShieldCheck,
  promote: CheckCircle2,
  deprecate: AlertTriangle,
  archive: Archive,
  supersede: Replace,
  redact: Eraser,
  merge: GitMerge,
}

const EVIDENCE_KINDS: EvidenceKindInput[] = [
  'incident_report',
  'pull_request',
  'commit',
  'dashboard',
  'log_query',
  'test_result',
  'jira_ticket',
  'confluence_page',
  'manual_note',
]

interface TrustActionsProps {
  recordId: string
  trustState: string | null | undefined
  riskClass: string | null | undefined
}

export function TrustActions({ recordId, trustState, riskClass }: TrustActionsProps) {
  const qc = useQueryClient()
  const [open, setOpen] = useState<TrustOp | null>(null)
  const ops = validOps(trustState)

  // Listen for trust_transitioned events on this record and invalidate the
  // parent memory query so the action set re-renders against the new state.
  const { last } = useEvents<AppEvent>({})
  useEffect(() => {
    if (!last) return
    if (last.kind === 'trust_transitioned' && last.id === recordId) {
      qc.invalidateQueries({ queryKey: memoryShowKey(recordId) })
    }
  }, [last, recordId, qc])

  function runReview() {
    fireMutation(() => postReview(recordId), 'reviewed', qc, recordId)
  }
  function runArchive() {
    fireMutation(() => postArchive(recordId), 'archived', qc, recordId)
  }

  if (ops.length === 0) {
    return (
      <div className="rounded-md border border-dashed border-border/60 px-3 py-2 text-xs text-muted-foreground">
        Trust state <code className="font-mono">{trustState ?? '—'}</code> is terminal.
        No further transitions are valid.
      </div>
    )
  }

  return (
    <div className="space-y-2" data-testid="trust-actions">
      <div className="flex items-center justify-between">
        <div className="font-mono text-xs uppercase tracking-wider text-muted-foreground">
          Trust ·{' '}
          <span className="text-foreground">{trustState ?? 'n/a'}</span>
          {isHighStakes(riskClass) && (
            <span className="ml-2 inline-flex items-center gap-1 text-amber-400">
              <AlertTriangle className="h-3 w-3" />
              high-stakes
            </span>
          )}
        </div>
      </div>
      <div className="flex flex-wrap gap-2">
        {ops.map((op) => {
          const Icon = ICON[op]
          // Review and archive can run immediately. Everything else collects
          // inputs in a dialog.
          const inline = op === 'review' || op === 'archive'
          return (
            <Button
              key={op}
              size="sm"
              variant={op === 'redact' ? 'destructive' : 'outline'}
              data-testid={`trust-op-${op}`}
              onClick={() => {
                if (op === 'review') return runReview()
                if (op === 'archive') return runArchive()
                setOpen(op)
              }}
              className={inline ? 'gap-2' : 'gap-2'}
            >
              <Icon className="h-3.5 w-3.5" />
              {OP_LABELS[op]}
            </Button>
          )
        })}
      </div>

      {open === 'promote' && (
        <PromoteDialog
          recordId={recordId}
          required={isHighStakes(riskClass)}
          onClose={() => setOpen(null)}
        />
      )}
      {open === 'deprecate' && (
        <ReasonDialog
          title="Deprecate record"
          submitLabel="Deprecate"
          recordId={recordId}
          fire={(reason) => postDeprecate(recordId, reason)}
          onClose={() => setOpen(null)}
        />
      )}
      {open === 'redact' && (
        <RedactAlertDialog recordId={recordId} onClose={() => setOpen(null)} />
      )}
      {open === 'supersede' && (
        <SupersedeDialog recordId={recordId} onClose={() => setOpen(null)} />
      )}
      {open === 'merge' && (
        <MergeDialog recordId={recordId} onClose={() => setOpen(null)} />
      )}
    </div>
  )
}

function fireMutation(
  run: () => Promise<{ record: RecordWire }>,
  label: string,
  qc: ReturnType<typeof useQueryClient>,
  recordId: string,
) {
  void run()
    .then(() => {
      toast.success(`Transitioned → ${label}`)
      qc.invalidateQueries({ queryKey: memoryShowKey(recordId) })
    })
    .catch((err) => {
      if (err instanceof ApiError && err.kind === 'conflict') {
        toast.error('Wrong source state — refresh and try again', {
          description: err.message,
        })
        qc.invalidateQueries({ queryKey: memoryShowKey(recordId) })
        return
      }
      toastApiError(err)
    })
}

function PromoteDialog({
  recordId,
  required,
  onClose,
}: {
  recordId: string
  required: boolean
  onClose: () => void
}) {
  const qc = useQueryClient()
  const [url, setUrl] = useState('')
  const [kind, setKind] = useState<EvidenceKindInput>('pull_request')
  const [reason, setReason] = useState('')
  const mut = useMutation({
    mutationFn: () =>
      postPromote(recordId, {
        reason: reason || undefined,
        evidenceUrl: url || undefined,
        evidenceType: url ? kind : undefined,
      }),
    onSuccess: () => {
      toast.success('Promoted')
      qc.invalidateQueries({ queryKey: memoryShowKey(recordId) })
      onClose()
    },
    onError: (err) => {
      if (err instanceof ApiError && err.kind === 'conflict') {
        toast.error('Wrong source state — refresh and try again')
        qc.invalidateQueries({ queryKey: memoryShowKey(recordId) })
        return
      }
      toastApiError(err)
    },
  })
  return (
    <Dialog open onOpenChange={(v) => !v && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle className="font-mono">Promote to verified</DialogTitle>
        </DialogHeader>
        <div className="space-y-3">
          {required && (
            <div className="rounded-md border border-amber-400/30 bg-amber-400/5 px-3 py-2 text-xs text-amber-300">
              High-stakes risk class — evidence URL is required.
            </div>
          )}
          <div className="space-y-1.5">
            <Label>Evidence URL{required && ' *'}</Label>
            <Input value={url} onChange={(e) => setUrl(e.target.value)} placeholder="https://…" />
          </div>
          <div className="space-y-1.5">
            <Label>Evidence kind</Label>
            <Select value={kind} onValueChange={(v) => setKind(v as EvidenceKindInput)}>
              <SelectTrigger>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {EVIDENCE_KINDS.map((k) => (
                  <SelectItem key={k} value={k}>
                    {k}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="space-y-1.5">
            <Label>Reason (optional)</Label>
            <Textarea
              value={reason}
              onChange={(e) => setReason(e.target.value)}
              rows={3}
              placeholder="Short rationale…"
            />
          </div>
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            Cancel
          </Button>
          <Button
            onClick={() => mut.mutate()}
            disabled={mut.isPending || (required && !url)}
            data-testid="promote-confirm"
          >
            {mut.isPending && <Loader2 className="mr-1 h-3 w-3 animate-spin" />}
            Promote
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function ReasonDialog({
  title,
  submitLabel,
  recordId,
  fire,
  onClose,
  variant = 'default',
}: {
  title: string
  submitLabel: string
  recordId: string
  fire: (reason: string) => Promise<{ record: RecordWire }>
  onClose: () => void
  variant?: 'default' | 'destructive'
}) {
  const qc = useQueryClient()
  const [reason, setReason] = useState('')
  const mut = useMutation({
    mutationFn: () => fire(reason),
    onSuccess: () => {
      toast.success(`${submitLabel} complete`)
      qc.invalidateQueries({ queryKey: memoryShowKey(recordId) })
      onClose()
    },
    onError: (err) => {
      if (err instanceof ApiError && err.kind === 'conflict') {
        toast.error('Wrong source state — refresh and try again')
        qc.invalidateQueries({ queryKey: memoryShowKey(recordId) })
        return
      }
      toastApiError(err)
    },
  })
  return (
    <Dialog open onOpenChange={(v) => !v && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle className="font-mono">{title}</DialogTitle>
        </DialogHeader>
        <div className="space-y-1.5">
          <Label>Reason *</Label>
          <Textarea
            value={reason}
            onChange={(e) => setReason(e.target.value)}
            rows={3}
            autoFocus
            placeholder="Why?"
          />
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            Cancel
          </Button>
          <Button
            variant={variant}
            onClick={() => mut.mutate()}
            disabled={!reason.trim() || mut.isPending}
          >
            {mut.isPending && <Loader2 className="mr-1 h-3 w-3 animate-spin" />}
            {submitLabel}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function SupersedeDialog({ recordId, onClose }: { recordId: string; onClose: () => void }) {
  const qc = useQueryClient()
  const [successor, setSuccessor] = useState('')
  const [reason, setReason] = useState('')
  const mut = useMutation({
    mutationFn: () => postSupersede(recordId, successor, reason || undefined),
    onSuccess: () => {
      toast.success('Superseded')
      qc.invalidateQueries({ queryKey: memoryShowKey(recordId) })
      onClose()
    },
    onError: (err) => toastApiError(err),
  })
  return (
    <Dialog open onOpenChange={(v) => !v && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle className="font-mono">Supersede record</DialogTitle>
        </DialogHeader>
        <p className="text-xs text-muted-foreground">
          Mark this record as superseded by another record (the successor).
        </p>
        <div className="space-y-1.5">
          <Label>Successor id *</Label>
          <Input value={successor} onChange={(e) => setSuccessor(e.target.value)} placeholder="memory:…" />
        </div>
        <div className="space-y-1.5">
          <Label>Reason</Label>
          <Textarea value={reason} onChange={(e) => setReason(e.target.value)} rows={2} />
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            Cancel
          </Button>
          <Button onClick={() => mut.mutate()} disabled={!successor.trim() || mut.isPending}>
            {mut.isPending && <Loader2 className="mr-1 h-3 w-3 animate-spin" />}
            Supersede
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function MergeDialog({ recordId, onClose }: { recordId: string; onClose: () => void }) {
  const qc = useQueryClient()
  const [raw, setRaw] = useState('')
  const [reason, setReason] = useState('')
  const [step, setStep] = useState<'inputs' | 'preview'>('inputs')
  const sources = raw
    .split(/[\s,]+/)
    .map((s) => s.trim())
    .filter(Boolean)
  const mut = useMutation({
    mutationFn: () => postMerge(recordId, sources, reason || undefined),
    onSuccess: (out) => {
      toast.success(`Merged ${out.count} record${out.count === 1 ? '' : 's'}`)
      qc.invalidateQueries({ queryKey: memoryShowKey(recordId) })
      onClose()
    },
    onError: (err) => toastApiError(err),
  })
  return (
    <Dialog open onOpenChange={(v) => !v && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle className="font-mono">
            {step === 'inputs'
              ? `Merge into ${recordId.slice(0, 14)}`
              : 'Confirm merge'}
          </DialogTitle>
        </DialogHeader>
        {step === 'inputs' ? (
          <>
            <p className="text-xs text-muted-foreground">
              Source ids are merged into this record. Sources are marked superseded.
            </p>
            <div className="space-y-1.5">
              <Label>Source ids *</Label>
              <Textarea
                value={raw}
                onChange={(e) => setRaw(e.target.value)}
                rows={3}
                placeholder="memory:… (comma or whitespace separated)"
              />
            </div>
            <div className="space-y-1.5">
              <Label>Reason</Label>
              <Textarea
                value={reason}
                onChange={(e) => setReason(e.target.value)}
                rows={2}
              />
            </div>
            <DialogFooter>
              <Button variant="outline" onClick={onClose}>
                Cancel
              </Button>
              <Button
                onClick={() => setStep('preview')}
                disabled={sources.length === 0}
                data-testid="merge-preview"
              >
                Preview merge
              </Button>
            </DialogFooter>
          </>
        ) : (
          <>
            <MergePreview canonicalId={recordId} sourceIds={sources} />
            {reason && (
              <p className="rounded-md border border-border/60 bg-background/60 px-3 py-2 text-xs">
                <span className="font-mono uppercase tracking-wider text-muted-foreground">
                  Reason:
                </span>{' '}
                {reason}
              </p>
            )}
            <DialogFooter>
              <Button variant="outline" onClick={() => setStep('inputs')}>
                Back
              </Button>
              <Button
                onClick={() => mut.mutate()}
                disabled={mut.isPending}
                data-testid="merge-confirm"
              >
                {mut.isPending && <Loader2 className="mr-1 h-3 w-3 animate-spin" />}
                Confirm merge
              </Button>
            </DialogFooter>
          </>
        )}
      </DialogContent>
    </Dialog>
  )
}

function MergePreview({
  canonicalId,
  sourceIds,
}: {
  canonicalId: string
  sourceIds: string[]
}) {
  return (
    <div className="space-y-3">
      <p className="text-xs text-muted-foreground">
        Review what will happen before applying. The merge is irreversible
        once submitted.
      </p>
      <div className="space-y-1">
        <div className="font-mono text-[0.625rem] uppercase tracking-wider text-muted-foreground">
          Canonical (kept)
        </div>
        <MergeRow id={canonicalId} highlight />
      </div>
      <div className="space-y-1">
        <div className="font-mono text-[0.625rem] uppercase tracking-wider text-muted-foreground">
          Superseded ({sourceIds.length}) — applied in order
        </div>
        <ul data-testid="merge-superseded" className="space-y-1">
          {sourceIds.map((sid, idx) => (
            <li key={sid} className="flex items-center gap-2">
              <span className="w-5 text-right font-mono text-[0.625rem] text-muted-foreground">
                {idx + 1}.
              </span>
              <MergeRow id={sid} />
            </li>
          ))}
        </ul>
      </div>
    </div>
  )
}

/**
 * Redact uses an AlertDialog (rather than the regular ReasonDialog) because
 * the action permanently wipes the record body and cannot be undone. The
 * confirmation requires a non-empty reason and forces the user past a
 * destructive-styled action button.
 */
function RedactAlertDialog({
  recordId,
  onClose,
}: {
  recordId: string
  onClose: () => void
}) {
  const qc = useQueryClient()
  const [reason, setReason] = useState('')
  const mut = useMutation({
    mutationFn: () => postRedact(recordId, reason),
    onSuccess: () => {
      toast.success('Record redacted')
      qc.invalidateQueries({ queryKey: memoryShowKey(recordId) })
      onClose()
    },
    onError: (err) => {
      if (err instanceof ApiError && err.kind === 'conflict') {
        toast.error('Wrong source state — refresh and try again')
        qc.invalidateQueries({ queryKey: memoryShowKey(recordId) })
        return
      }
      toastApiError(err)
    },
  })
  return (
    <AlertDialog open onOpenChange={(v) => !v && onClose()}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle className="font-mono">Redact record?</AlertDialogTitle>
          <AlertDialogDescription>
            This permanently wipes the record body and cannot be undone. The
            tombstone remains in the history chain, but the original content is
            unrecoverable.
          </AlertDialogDescription>
        </AlertDialogHeader>
        <div className="space-y-1.5">
          <Label>Reason *</Label>
          <Textarea
            value={reason}
            onChange={(e) => setReason(e.target.value)}
            rows={3}
            autoFocus
            placeholder="Why is redaction required?"
          />
        </div>
        <AlertDialogFooter>
          <AlertDialogCancel onClick={onClose}>Cancel</AlertDialogCancel>
          <AlertDialogAction
            data-testid="redact-confirm"
            disabled={!reason.trim() || mut.isPending}
            onClick={(e) => {
              e.preventDefault()
              mut.mutate()
            }}
            className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
          >
            {mut.isPending && <Loader2 className="mr-1 h-3 w-3 animate-spin" />}
            Redact
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}

function MergeRow({ id, highlight = false }: { id: string; highlight?: boolean }) {
  const q = useMemoryQuery(id)
  const title = q.data?.record?.envelope?.title ?? '…'
  return (
    <div
      className={
        'flex items-center gap-2 rounded-md border px-3 py-1.5 ' +
        (highlight ? 'border-primary/40 bg-primary/5' : 'border-border/70 bg-background/60')
      }
    >
      <span className="truncate text-sm">{title}</span>
      <span className="ml-auto truncate font-mono text-[0.625rem] text-muted-foreground">
        {id}
      </span>
    </div>
  )
}

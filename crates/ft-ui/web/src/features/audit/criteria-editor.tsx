/**
 * Acceptance criteria editor. List of toggles with evidence attachment. The
 * toggle PATCH is optimistic — we update the cached `CriteriaListOutput`
 * immediately and rollback on error.
 */
import { useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Loader2, Plus, Paperclip, Check } from 'lucide-react'
import type { CriteriaListOutput } from '@/api/types/CriteriaListOutput'
import type { EvidenceKindInput } from '@/api/types/EvidenceKindInput'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Skeleton } from '@/components/ui/skeleton'
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { toastApiError } from '@/api/error'
import { cn } from '@/lib/utils'
import {
  addCriterion,
  attachCriterionEvidence,
  fetchCriteria,
  toggleCriterion,
} from './api'

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

const criteriaKey = (id: string) => ['audit-criteria', id] as const

export function CriteriaEditor({ recordId }: { recordId: string }) {
  const qc = useQueryClient()
  const { data, isLoading, error } = useQuery({
    queryKey: criteriaKey(recordId),
    queryFn: () => fetchCriteria(recordId),
    enabled: !!recordId,
  })

  const toggle = useMutation({
    mutationFn: ({ which, checked }: { which: string; checked: boolean }) =>
      toggleCriterion(recordId, which, checked),
    onMutate: async ({ which, checked }) => {
      await qc.cancelQueries({ queryKey: criteriaKey(recordId) })
      const previous = qc.getQueryData<CriteriaListOutput>(criteriaKey(recordId))
      qc.setQueryData<CriteriaListOutput>(criteriaKey(recordId), (prev) => {
        if (!prev) return prev
        return {
          ...prev,
          items: prev.items.map((it) =>
            it.id === which || String(it.index) === which ? { ...it, checked } : it,
          ),
        }
      })
      return { previous }
    },
    onError: (err, _vars, ctx) => {
      if (ctx?.previous) qc.setQueryData(criteriaKey(recordId), ctx.previous)
      toastApiError(err, 'Toggle failed')
    },
    onSuccess: (out) => {
      qc.setQueryData(criteriaKey(recordId), out)
    },
  })

  const add = useMutation({
    mutationFn: (text: string) => addCriterion(recordId, text),
    onSuccess: (out) => qc.setQueryData(criteriaKey(recordId), out),
    onError: (err) => toastApiError(err, 'Add failed'),
  })

  const [draft, setDraft] = useState('')
  const [evidence, setEvidence] = useState<{ which: string } | null>(null)

  if (isLoading) return <Skeleton className="h-32 w-full" />
  if (error) {
    return (
      <p className="text-sm text-destructive">
        Failed to load criteria: {(error as Error).message}
      </p>
    )
  }
  if (!data) return null

  return (
    <div className="space-y-3" data-testid="criteria-editor">
      <h3 className="text-sm font-medium uppercase tracking-wide text-muted-foreground">
        Acceptance criteria
      </h3>
      {data.items.length === 0 ? (
        <p className="rounded-[var(--radius)] border border-dashed border-border px-3 py-3 text-sm text-muted-foreground">
          No acceptance criteria yet.
        </p>
      ) : (
        <ul className="space-y-2">
          {data.items.map((it) => (
            <li
              key={it.id}
              className="flex items-start gap-3 rounded-[var(--radius)] border border-border bg-card p-3 transition-colors hover:bg-surface-2"
            >
              <button
                type="button"
                aria-label={`Toggle ${it.id}`}
                onClick={() => toggle.mutate({ which: it.id, checked: !it.checked })}
                className={cn(
                  'mt-0.5 flex h-4 w-4 flex-none items-center justify-center rounded border border-border',
                  it.checked && 'border-primary bg-primary text-primary-foreground',
                )}
                data-testid={`criterion-${it.id}`}
              >
                {it.checked && <Check className="h-3 w-3" />}
              </button>
              <div className="flex-1">
                <div className="text-sm">
                  <code className="mr-2 font-mono text-[0.65rem] text-muted-foreground">
                    {it.id}
                  </code>
                  {it.text}
                </div>
                {it.evidenceUrl && (
                  <a
                    href={it.evidenceUrl}
                    target="_blank"
                    rel="noreferrer noopener"
                    className="mt-0.5 inline-flex items-center gap-1 font-mono text-[0.65rem] text-primary hover:underline"
                  >
                    <Paperclip className="h-3 w-3" />
                    evidence
                  </a>
                )}
              </div>
              <Button
                type="button"
                size="sm"
                variant="ghost"
                className="h-7 gap-1 text-xs"
                onClick={() => setEvidence({ which: it.id })}
              >
                <Paperclip className="h-3 w-3" />
                Attach
              </Button>
            </li>
          ))}
        </ul>
      )}

      <form
        className="flex gap-2"
        onSubmit={(e) => {
          e.preventDefault()
          if (!draft.trim()) return
          add.mutate(draft.trim())
          setDraft('')
        }}
      >
        <Input value={draft} onChange={(e) => setDraft(e.target.value)} placeholder="Add a criterion…" />
        <Button type="submit" size="sm" disabled={!draft.trim() || add.isPending} className="gap-1.5">
          {add.isPending ? <Loader2 className="h-3 w-3 animate-spin" /> : <Plus className="h-3 w-3" />}
          Add
        </Button>
      </form>

      {evidence && (
        <EvidenceDialog
          recordId={recordId}
          which={evidence.which}
          onClose={() => setEvidence(null)}
        />
      )}
    </div>
  )
}

function EvidenceDialog({
  recordId,
  which,
  onClose,
}: {
  recordId: string
  which: string
  onClose: () => void
}) {
  const qc = useQueryClient()
  const [url, setUrl] = useState('')
  const [kind, setKind] = useState<EvidenceKindInput>('pull_request')
  const mut = useMutation({
    mutationFn: () => attachCriterionEvidence(recordId, which, { url, kind }),
    onSuccess: (out) => {
      qc.setQueryData(criteriaKey(recordId), out)
      onClose()
    },
    onError: (err) => toastApiError(err, 'Attach failed'),
  })
  return (
    <Dialog open onOpenChange={(v) => !v && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle className="font-mono">Attach evidence to {which}</DialogTitle>
        </DialogHeader>
        <div className="space-y-3">
          <div className="space-y-1.5">
            <Label>URL *</Label>
            <Input value={url} onChange={(e) => setUrl(e.target.value)} placeholder="https://…" />
          </div>
          <div className="space-y-1.5">
            <Label>Kind</Label>
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
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            Cancel
          </Button>
          <Button onClick={() => mut.mutate()} disabled={!url || mut.isPending}>
            {mut.isPending && <Loader2 className="mr-1 h-3 w-3 animate-spin" />}
            Attach
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

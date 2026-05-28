/**
 * Salvage queue UI — the marquee replacement for the CLI's interactive
 * prompts.
 *
 * Two-step flow:
 *   1. Discover. "Run salvage scan" → POST /api/memory/salvage { dryRun: true }
 *      returns candidate entries. Each is checkbox-selectable.
 *   2. Apply.    "Accept selected" → confirmation dialog →
 *      POST /api/memory/salvage { dryRun: false, selected: [ids] }.
 *      The server emits one `memory_salvaged` event per id.
 *
 * Empty dry-run results render a "your memory is up to date" affordance
 * so users aren't dropped into a blank queue.
 */
import { useMemo, useState } from 'react'
import { Loader2, ShieldCheck, RefreshCcw } from 'lucide-react'
import type { SalvageEntry } from '@/api/types/SalvageEntry'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Separator } from '@/components/ui/separator'
import { cn } from '@/lib/utils'
import { useSalvageApply, useSalvageDryRun } from './use-memory-mutations'

export function SalvageQueue() {
  const dryRun = useSalvageDryRun()
  const apply = useSalvageApply()

  const [base, setBase] = useState('main')
  const [branch, setBranch] = useState('')
  const [selected, setSelected] = useState<Set<string>>(new Set())
  const [filter, setFilter] = useState('')
  const [confirmOpen, setConfirmOpen] = useState(false)

  const entries: SalvageEntry[] = dryRun.data?.entries ?? []
  const filtered = useMemo(() => {
    const q = filter.trim().toLowerCase()
    if (!q) return entries
    return entries.filter(
      (e) =>
        e.id.toLowerCase().includes(q) ||
        e.kind.toLowerCase().includes(q) ||
        e.path.toLowerCase().includes(q) ||
        e.reason.toLowerCase().includes(q),
    )
  }, [entries, filter])

  function toggle(id: string) {
    setSelected((prev) => {
      const next = new Set(prev)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })
  }

  async function runScan() {
    setSelected(new Set())
    await dryRun.mutateAsync({ base, branch: branch || null })
  }

  async function applySelected() {
    setConfirmOpen(false)
    await apply.mutateAsync({
      base,
      branch: branch || null,
      selected: Array.from(selected),
    })
    setSelected(new Set())
    // Re-scan so the queue reflects the new repo state.
    await dryRun.mutateAsync({ base, branch: branch || null })
  }

  const hasScanned = !!dryRun.data
  const isEmpty = hasScanned && entries.length === 0

  return (
    <div className="mx-auto max-w-4xl space-y-4 p-6">
      <header className="flex flex-wrap items-end justify-between gap-3">
        <div>
          <h1 className="font-mono text-lg font-semibold tracking-tight">Salvage queue</h1>
          <p className="text-xs text-muted-foreground">
            Plan-then-apply. Nothing mutates until you click <em>Accept</em>.
          </p>
        </div>
        <div className="flex flex-wrap items-end gap-2">
          <div className="space-y-1">
            <Label className="text-xs">Base branch</Label>
            <Input
              className="h-8 w-32 text-xs"
              value={base}
              onChange={(e) => setBase(e.target.value)}
            />
          </div>
          <div className="space-y-1">
            <Label className="text-xs">Source branch</Label>
            <Input
              className="h-8 w-40 text-xs"
              value={branch}
              onChange={(e) => setBranch(e.target.value)}
              placeholder="(current branch)"
            />
          </div>
          <Button
            size="sm"
            onClick={runScan}
            disabled={dryRun.isPending}
            className="gap-2"
          >
            {dryRun.isPending ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              <RefreshCcw className="h-4 w-4" />
            )}
            Run salvage scan
          </Button>
        </div>
      </header>

      {dryRun.error ? (
        <div className="text-sm text-destructive">
          Salvage scan failed: {(dryRun.error as Error).message}
        </div>
      ) : null}

      {!hasScanned && !dryRun.isPending && (
        <div className="rounded-md border border-dashed border-border/60 px-4 py-8 text-center text-sm text-muted-foreground">
          Click <em>Run salvage scan</em> to compute candidates. The scan is
          read-only.
        </div>
      )}

      {isEmpty && (
        <div className="flex items-center gap-2 rounded-md border border-primary/30 bg-primary/5 px-4 py-3 text-sm">
          <ShieldCheck className="h-4 w-4 text-primary" />
          No salvage candidates — your memory is up to date.
        </div>
      )}

      {entries.length > 0 && (
        <>
          <Separator />
          <div className="flex flex-wrap items-center justify-between gap-3">
            <Input
              className="h-8 max-w-sm text-xs"
              placeholder="Filter by id / kind / path / reason"
              value={filter}
              onChange={(e) => setFilter(e.target.value)}
            />
            <div className="flex items-center gap-2">
              <Button
                size="sm"
                variant="ghost"
                onClick={() => setSelected(new Set(filtered.map((e) => e.id)))}
              >
                Select all
              </Button>
              <Button size="sm" variant="ghost" onClick={() => setSelected(new Set())}>
                Clear
              </Button>
              <Button
                size="sm"
                disabled={selected.size === 0 || apply.isPending}
                onClick={() => setConfirmOpen(true)}
                className="gap-2"
              >
                {apply.isPending && <Loader2 className="h-4 w-4 animate-spin" />}
                Accept selected ({selected.size})
              </Button>
            </div>
          </div>

          <ul data-testid="salvage-entries" className="space-y-2">
            {filtered.map((entry) => (
              <li
                key={entry.id}
                className={cn(
                  'flex items-start gap-3 rounded-md border border-border/70 bg-background/80 p-3',
                  selected.has(entry.id) && 'border-primary/60 bg-primary/5',
                )}
              >
                <Input
                  type="checkbox"
                  className="mt-1 h-4 w-4 accent-primary"
                  checked={selected.has(entry.id)}
                  onChange={() => toggle(entry.id)}
                  aria-label={`Select ${entry.id}`}
                />
                <div className="flex-1 space-y-1">
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="rounded-sm bg-primary/15 px-1.5 py-0.5 font-mono text-[0.625rem] font-semibold uppercase tracking-wider text-primary">
                      {entry.kind}
                    </span>
                    <span className="font-mono text-xs text-muted-foreground">
                      {entry.id}
                    </span>
                    <span
                      className={cn(
                        'rounded px-1.5 py-0.5 text-[0.625rem] font-mono uppercase tracking-wider',
                        entry.action === 'salvaged'
                          ? 'bg-primary/20 text-primary'
                          : 'bg-muted text-muted-foreground',
                      )}
                    >
                      {entry.action}
                    </span>
                  </div>
                  <div className="text-xs text-muted-foreground">
                    <span className="font-mono">{entry.path}</span>
                  </div>
                  <div className="text-xs">{entry.reason}</div>
                </div>
              </li>
            ))}
          </ul>
        </>
      )}

      <Dialog open={confirmOpen} onOpenChange={setConfirmOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle className="font-mono">Apply salvage?</DialogTitle>
          </DialogHeader>
          <p className="text-sm text-muted-foreground">
            Apply salvage to {selected.size}{' '}
            {selected.size === 1 ? 'entry' : 'entries'}? This mutates the
            workspace and emits one <code className="font-mono">memory_salvaged</code> event
            per record.
          </p>
          <DialogFooter>
            <Button variant="ghost" onClick={() => setConfirmOpen(false)}>
              Cancel
            </Button>
            <Button onClick={applySelected} disabled={apply.isPending} className="gap-2">
              {apply.isPending && <Loader2 className="h-4 w-4 animate-spin" />}
              Apply
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  )
}

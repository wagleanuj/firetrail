/**
 * Create-memory dialog. Tabs for the six kinds (incident / finding / runbook
 * / decision / gotcha / memory); each tab mounts its own form component
 * with a kind-specific zod schema.
 *
 * Mirrors the W1-C tickets create-dialog UX: shadcn `<Dialog>` + `<Tabs>`,
 * Markdown editor inline, optimistic insert into the list cache on success.
 */
import { useState } from 'react'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Tabs, TabsList, TabsTrigger, TabsContent } from '@/components/ui/tabs'
import { IncidentForm } from './forms/incident'
import { FindingForm } from './forms/finding'
import { RunbookForm } from './forms/runbook'
import { DecisionForm } from './forms/decision'
import { GotchaForm } from './forms/gotcha'
import { MemoryForm } from './forms/memory'
import { MEMORY_KINDS, type MemoryCreateKind } from './types'

interface CreateMemoryDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  initialKind?: MemoryCreateKind
}

const LABELS: Record<MemoryCreateKind, string> = {
  incident: 'Incident',
  finding: 'Finding',
  runbook: 'Runbook',
  decision: 'Decision',
  gotcha: 'Gotcha',
  memory: 'Memory',
}

export function CreateMemoryDialog({
  open,
  onOpenChange,
  initialKind = 'memory',
}: CreateMemoryDialogProps) {
  const [kind, setKind] = useState<MemoryCreateKind>(initialKind)
  const close = () => onOpenChange(false)

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl">
        <DialogHeader>
          <DialogTitle className="font-mono">New memory</DialogTitle>
        </DialogHeader>
        <Tabs value={kind} onValueChange={(v) => setKind(v as MemoryCreateKind)}>
          <TabsList className="flex flex-wrap">
            {MEMORY_KINDS.map((k) => (
              <TabsTrigger key={k} value={k}>
                {LABELS[k]}
              </TabsTrigger>
            ))}
          </TabsList>
          <TabsContent value="incident">
            <IncidentForm onDone={close} />
          </TabsContent>
          <TabsContent value="finding">
            <FindingForm onDone={close} />
          </TabsContent>
          <TabsContent value="runbook">
            <RunbookForm onDone={close} />
          </TabsContent>
          <TabsContent value="decision">
            <DecisionForm onDone={close} />
          </TabsContent>
          <TabsContent value="gotcha">
            <GotchaForm onDone={close} />
          </TabsContent>
          <TabsContent value="memory">
            <MemoryForm onDone={close} />
          </TabsContent>
        </Tabs>
      </DialogContent>
    </Dialog>
  )
}

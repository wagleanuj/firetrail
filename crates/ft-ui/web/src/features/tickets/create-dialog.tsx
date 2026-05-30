/**
 * Create-ticket dialog. Kind tabs (task/epic/subtask/bug), zod-validated form,
 * Tiptap description.
 *
 * Each kind has its own zod schema mirroring the relevant `Create*Input` from
 * ts-rs (minus `request_id`, which the fetch client mints automatically). The
 * union here is intentionally hand-written: the ts-rs types are too permissive
 * (every field is `T | null`) for direct use as a form schema.
 */
import { useState } from 'react'
import { z } from 'zod'
import { useForm, type SubmitHandler } from 'react-hook-form'
import { zodResolver } from '@hookform/resolvers/zod'
import { Loader2 } from 'lucide-react'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from '@/components/ui/dialog'
import { Tabs, TabsList, TabsTrigger, TabsContent } from '@/components/ui/tabs'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Button } from '@/components/ui/button'
import { OwnerCombobox, ScopeCombobox } from '@/components/ui/autocomplete'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { DescriptionEditor } from './description-editor'
import { useCreateTicket } from './use-ticket-mutations'
import type { CreateBody } from './api'

type Kind = 'task' | 'epic' | 'subtask' | 'bug'

const PRIORITIES = ['p0', 'p1', 'p2', 'p3', 'p4'] as const

const labelsField = z
  .string()
  .optional()
  .refine(
    (s) => {
      const parts = (s ?? '')
        .split(',')
        .map((x) => x.trim())
        .filter(Boolean)
      return parts.every((l) => l.includes('='))
    },
    { message: 'labels must be key=value, comma-separated' },
  )

function parseLabels(s: string | undefined): string[] {
  return (s ?? '')
    .split(',')
    .map((x) => x.trim())
    .filter(Boolean)
}

const taskSchema = z.object({
  title: z.string().min(1, 'Title is required'),
  description: z.string().optional().default(''),
  epic: z.string().optional().default(''),
  priority: z.enum(PRIORITIES).optional(),
  owner: z.string().optional().default(''),
  scope: z.string().optional().default(''),
  labels: labelsField,
})

const epicSchema = z.object({
  title: z.string().min(1, 'Title is required'),
  description: z.string().optional().default(''),
  priority: z.enum(PRIORITIES).optional(),
  scope: z.string().optional().default(''),
  labels: labelsField,
})

const subtaskSchema = z.object({
  title: z.string().min(1, 'Title is required'),
  parent: z.string().min(1, 'Parent task id is required'),
  description: z.string().optional().default(''),
  priority: z.enum(PRIORITIES).optional(),
  owner: z.string().optional().default(''),
  scope: z.string().optional().default(''),
  labels: labelsField,
})

const bugSchema = z.object({
  title: z.string().min(1, 'Title is required'),
  description: z.string().optional().default(''),
  service: z.string().optional().default(''),
  severity: z.string().optional().default(''),
  priority: z.enum(PRIORITIES).optional(),
  scope: z.string().optional().default(''),
  labels: labelsField,
})

// Exported for unit tests — verifies title-required validation.
export const SCHEMAS = {
  task: taskSchema,
  epic: epicSchema,
  subtask: subtaskSchema,
  bug: bugSchema,
} as const

interface CreateDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

export function CreateDialog({ open, onOpenChange }: CreateDialogProps) {
  const [kind, setKind] = useState<Kind>('task')
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl">
        <DialogHeader>
          <DialogTitle className="font-mono">New ticket</DialogTitle>
          <DialogDescription className="sr-only">
            Create a new ticket.
          </DialogDescription>
        </DialogHeader>
        <Tabs value={kind} onValueChange={(v) => setKind(v as Kind)}>
          <TabsList>
            <TabsTrigger value="task">Task</TabsTrigger>
            <TabsTrigger value="epic">Epic</TabsTrigger>
            <TabsTrigger value="subtask">Subtask</TabsTrigger>
            <TabsTrigger value="bug">Bug</TabsTrigger>
          </TabsList>
          <TabsContent value="task">
            <TaskForm onDone={() => onOpenChange(false)} />
          </TabsContent>
          <TabsContent value="epic">
            <EpicForm onDone={() => onOpenChange(false)} />
          </TabsContent>
          <TabsContent value="subtask">
            <SubtaskForm onDone={() => onOpenChange(false)} />
          </TabsContent>
          <TabsContent value="bug">
            <BugForm onDone={() => onOpenChange(false)} />
          </TabsContent>
        </Tabs>
      </DialogContent>
    </Dialog>
  )
}

function FormShell({ children, submitting }: { children: React.ReactNode; submitting: boolean }) {
  return (
    <div className="space-y-4">
      {children}
      <DialogFooter>
        <Button type="submit" form="create-ticket-form" disabled={submitting} className="gap-2">
          {submitting && <Loader2 className="h-4 w-4 animate-spin" />}
          Create
        </Button>
      </DialogFooter>
    </div>
  )
}

function PrioritySelect({ value, onChange }: { value?: string; onChange: (v: string | undefined) => void }) {
  return (
    <Select value={value} onValueChange={(v) => onChange(v || undefined)}>
      <SelectTrigger>
        <SelectValue placeholder="Priority" />
      </SelectTrigger>
      <SelectContent>
        {PRIORITIES.map((p) => (
          <SelectItem key={p} value={p}>
            {p.toUpperCase()}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  )
}

type TaskValues = z.input<typeof taskSchema>
function TaskForm({ onDone }: { onDone: () => void }) {
  const mutate = useCreateTicket()
  const form = useForm<TaskValues>({
    resolver: zodResolver(taskSchema) as never,
    defaultValues: { title: '', description: '', epic: '', owner: '', scope: '', labels: '' },
  })
  const onSubmit: SubmitHandler<TaskValues> = async (v) => {
    const body: CreateBody = {
      kind: 'task',
      title: v.title,
      description: v.description || null,
      epic: v.epic || null,
      priority: v.priority ?? null,
      owner: v.owner || null,
      scope: v.scope || null,
      labels: parseLabels(v.labels),
    }
    await mutate.mutateAsync(body)
    onDone()
  }
  return (
    <form id="create-ticket-form" onSubmit={form.handleSubmit(onSubmit)}>
      <FormShell submitting={mutate.isPending}>
        <Field label="Title" error={form.formState.errors.title?.message}>
          <Input autoFocus {...form.register('title')} placeholder="Short, imperative" />
        </Field>
        <Field label="Description">
          <DescriptionEditor
            value={form.watch('description') ?? ''}
            onChange={(md) => form.setValue('description', md)}
          />
        </Field>
        <div className="grid grid-cols-2 gap-3">
          <Field label="Epic">
            <Input {...form.register('epic')} placeholder="epic id (optional)" />
          </Field>
          <Field label="Priority">
            <PrioritySelect
              value={form.watch('priority')}
              onChange={(v) => form.setValue('priority', v as (typeof PRIORITIES)[number] | undefined)}
            />
          </Field>
          <Field label="Owner">
            <OwnerCombobox
              value={form.watch('owner') ?? ''}
              onValueChange={(v) => form.setValue('owner', v)}
            />
          </Field>
          <Field label="Scope">
            <ScopeCombobox
              value={form.watch('scope') ?? ''}
              onValueChange={(v) => form.setValue('scope', v)}
            />
          </Field>
        </div>
        <Field label="Labels" error={form.formState.errors.labels?.message}>
          <Input {...form.register('labels')} placeholder="key=value, key=value" />
        </Field>
      </FormShell>
    </form>
  )
}

type EpicValues = z.input<typeof epicSchema>
function EpicForm({ onDone }: { onDone: () => void }) {
  const mutate = useCreateTicket()
  const form = useForm<EpicValues>({
    resolver: zodResolver(epicSchema) as never,
    defaultValues: { title: '', description: '', scope: '', labels: '' },
  })
  const onSubmit: SubmitHandler<EpicValues> = async (v) => {
    await mutate.mutateAsync({
      kind: 'epic',
      title: v.title,
      description: v.description || null,
      priority: v.priority ?? null,
      scope: v.scope || null,
      labels: parseLabels(v.labels),
    })
    onDone()
  }
  return (
    <form id="create-ticket-form" onSubmit={form.handleSubmit(onSubmit)}>
      <FormShell submitting={mutate.isPending}>
        <Field label="Title" error={form.formState.errors.title?.message}>
          <Input autoFocus {...form.register('title')} />
        </Field>
        <Field label="Description">
          <DescriptionEditor
            value={form.watch('description') ?? ''}
            onChange={(md) => form.setValue('description', md)}
          />
        </Field>
        <div className="grid grid-cols-2 gap-3">
          <Field label="Priority">
            <PrioritySelect
              value={form.watch('priority')}
              onChange={(v) => form.setValue('priority', v as (typeof PRIORITIES)[number] | undefined)}
            />
          </Field>
          <Field label="Scope">
            <ScopeCombobox
              value={form.watch('scope') ?? ''}
              onValueChange={(v) => form.setValue('scope', v)}
            />
          </Field>
        </div>
        <Field label="Labels" error={form.formState.errors.labels?.message}>
          <Input {...form.register('labels')} placeholder="key=value, key=value" />
        </Field>
      </FormShell>
    </form>
  )
}

type SubtaskValues = z.input<typeof subtaskSchema>
function SubtaskForm({ onDone }: { onDone: () => void }) {
  const mutate = useCreateTicket()
  const form = useForm<SubtaskValues>({
    resolver: zodResolver(subtaskSchema) as never,
    defaultValues: { title: '', parent: '', description: '', owner: '', scope: '', labels: '' },
  })
  const onSubmit: SubmitHandler<SubtaskValues> = async (v) => {
    await mutate.mutateAsync({
      kind: 'subtask',
      title: v.title,
      parent: v.parent,
      description: v.description || null,
      priority: v.priority ?? null,
      owner: v.owner || null,
      scope: v.scope || null,
      labels: parseLabels(v.labels),
    })
    onDone()
  }
  return (
    <form id="create-ticket-form" onSubmit={form.handleSubmit(onSubmit)}>
      <FormShell submitting={mutate.isPending}>
        <Field label="Title" error={form.formState.errors.title?.message}>
          <Input autoFocus {...form.register('title')} />
        </Field>
        <Field label="Parent task id" error={form.formState.errors.parent?.message}>
          <Input {...form.register('parent')} placeholder="task:…" />
        </Field>
        <Field label="Description">
          <DescriptionEditor
            value={form.watch('description') ?? ''}
            onChange={(md) => form.setValue('description', md)}
          />
        </Field>
        <div className="grid grid-cols-2 gap-3">
          <Field label="Priority">
            <PrioritySelect
              value={form.watch('priority')}
              onChange={(v) => form.setValue('priority', v as (typeof PRIORITIES)[number] | undefined)}
            />
          </Field>
          <Field label="Owner">
            <OwnerCombobox
              value={form.watch('owner') ?? ''}
              onValueChange={(v) => form.setValue('owner', v)}
            />
          </Field>
        </div>
        <Field label="Scope">
          <ScopeCombobox
            value={form.watch('scope') ?? ''}
            onValueChange={(v) => form.setValue('scope', v)}
          />
        </Field>
        <Field label="Labels" error={form.formState.errors.labels?.message}>
          <Input {...form.register('labels')} placeholder="key=value, key=value" />
        </Field>
      </FormShell>
    </form>
  )
}

type BugValues = z.input<typeof bugSchema>
function BugForm({ onDone }: { onDone: () => void }) {
  const mutate = useCreateTicket()
  const form = useForm<BugValues>({
    resolver: zodResolver(bugSchema) as never,
    defaultValues: { title: '', description: '', service: '', severity: '', scope: '', labels: '' },
  })
  const onSubmit: SubmitHandler<BugValues> = async (v) => {
    await mutate.mutateAsync({
      kind: 'bug',
      title: v.title,
      description: v.description || null,
      service: v.service || null,
      severity: v.severity || null,
      priority: v.priority ?? null,
      scope: v.scope || null,
      labels: parseLabels(v.labels),
    })
    onDone()
  }
  return (
    <form id="create-ticket-form" onSubmit={form.handleSubmit(onSubmit)}>
      <FormShell submitting={mutate.isPending}>
        <Field label="Title" error={form.formState.errors.title?.message}>
          <Input autoFocus {...form.register('title')} />
        </Field>
        <Field label="Description">
          <DescriptionEditor
            value={form.watch('description') ?? ''}
            onChange={(md) => form.setValue('description', md)}
          />
        </Field>
        <div className="grid grid-cols-2 gap-3">
          <Field label="Service">
            <Input {...form.register('service')} />
          </Field>
          <Field label="Severity">
            <Input {...form.register('severity')} placeholder="sev1 / sev2 / sev3" />
          </Field>
          <Field label="Priority">
            <PrioritySelect
              value={form.watch('priority')}
              onChange={(v) => form.setValue('priority', v as (typeof PRIORITIES)[number] | undefined)}
            />
          </Field>
          <Field label="Scope">
            <ScopeCombobox
              value={form.watch('scope') ?? ''}
              onValueChange={(v) => form.setValue('scope', v)}
            />
          </Field>
        </div>
        <Field label="Labels" error={form.formState.errors.labels?.message}>
          <Input {...form.register('labels')} placeholder="key=value, key=value" />
        </Field>
      </FormShell>
    </form>
  )
}

function Field({
  label,
  error,
  children,
}: {
  label: string
  error?: string
  children: React.ReactNode
}) {
  return (
    <div className="space-y-1.5">
      <Label>{label}</Label>
      {children}
      {error && <p className="text-xs text-destructive">{error}</p>}
    </div>
  )
}

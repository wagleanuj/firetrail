import { useForm, type SubmitHandler } from 'react-hook-form'
import { zodResolver } from '@hookform/resolvers/zod'
import { Input } from '@/components/ui/input'
import { ScopeCombobox } from '@/components/ui/autocomplete'
import { useCreateMemory } from '../use-memory-mutations'
import { runbookSchema, type RunbookValues } from '../schemas'
import { Field, FormShell, RiskClassSelect } from './shared'

const FORM_ID = 'create-memory-runbook'

export function RunbookForm({ onDone }: { onDone: () => void }) {
  const mutate = useCreateMemory()
  const form = useForm<RunbookValues>({
    resolver: zodResolver(runbookSchema) as never,
    defaultValues: { title: '', summary: '', appliesTo: '', scope: '' },
  })

  const onSubmit: SubmitHandler<RunbookValues> = async (v) => {
    const parsed = runbookSchema.parse(v)
    await mutate.mutateAsync({
      kind: 'runbook',
      title: parsed.title,
      summary: parsed.summary,
      appliesTo: parsed.appliesTo,
      riskClass: parsed.riskClass ?? null,
      scope: parsed.scope || null,
    })
    onDone()
  }

  return (
    <form id={FORM_ID} onSubmit={form.handleSubmit(onSubmit)}>
      <FormShell submitting={mutate.isPending} formId={FORM_ID}>
        <Field label="Title" error={form.formState.errors.title?.message}>
          <Input autoFocus {...form.register('title')} placeholder="Restart the api service" />
        </Field>
        <Field label="Summary" error={form.formState.errors.summary?.message}>
          <Input {...form.register('summary')} placeholder="When to use this runbook" />
        </Field>
        <Field label="Applies to (comma-separated services)">
          <Input {...form.register('appliesTo')} placeholder="api, worker" />
        </Field>
        <div className="grid grid-cols-2 gap-3">
          <Field label="Risk class">
            <RiskClassSelect
              value={form.watch('riskClass')}
              onChange={(v) =>
                form.setValue('riskClass', v as RunbookValues['riskClass'])
              }
            />
          </Field>
          <Field label="Scope">
            <ScopeCombobox
              value={form.watch('scope') ?? ''}
              onValueChange={(v) => form.setValue('scope', v)}
            />
          </Field>
        </div>
      </FormShell>
    </form>
  )
}

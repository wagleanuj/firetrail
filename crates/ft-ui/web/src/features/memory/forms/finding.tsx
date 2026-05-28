import { useForm, type SubmitHandler } from 'react-hook-form'
import { zodResolver } from '@hookform/resolvers/zod'
import { Input } from '@/components/ui/input'
import { MarkdownEditor } from '@/components/markdown-editor'
import { useCreateMemory } from '../use-memory-mutations'
import { findingSchema, type FindingValues } from '../schemas'
import { Field, FormShell, RiskClassSelect } from './shared'

const FORM_ID = 'create-memory-finding'

export function FindingForm({ onDone }: { onDone: () => void }) {
  const mutate = useCreateMemory()
  const form = useForm<FindingValues>({
    resolver: zodResolver(findingSchema) as never,
    defaultValues: { summary: '', incident: '', details: '', affected: '', scope: '' },
  })

  const onSubmit: SubmitHandler<FindingValues> = async (v) => {
    const parsed = findingSchema.parse(v)
    await mutate.mutateAsync({
      kind: 'finding',
      summary: parsed.summary,
      incident: parsed.incident || null,
      details: parsed.details || null,
      affected: parsed.affected,
      riskClass: parsed.riskClass ?? null,
      scope: parsed.scope || null,
    })
    onDone()
  }

  return (
    <form id={FORM_ID} onSubmit={form.handleSubmit(onSubmit)}>
      <FormShell submitting={mutate.isPending} formId={FORM_ID}>
        <Field label="Summary" error={form.formState.errors.summary?.message}>
          <Input autoFocus {...form.register('summary')} placeholder="What did you find?" />
        </Field>
        <Field label="Incident id">
          <Input {...form.register('incident')} placeholder="incident:… (optional)" />
        </Field>
        <Field label="Details (Markdown)">
          <MarkdownEditor
            value={form.watch('details') ?? ''}
            onChange={(md) => form.setValue('details', md)}
            placeholder="Repro, evidence, mitigation"
          />
        </Field>
        <div className="grid grid-cols-2 gap-3">
          <Field label="Risk class">
            <RiskClassSelect
              value={form.watch('riskClass')}
              onChange={(v) =>
                form.setValue('riskClass', v as FindingValues['riskClass'])
              }
            />
          </Field>
          <Field label="Scope">
            <Input {...form.register('scope')} />
          </Field>
        </div>
        <Field label="Affected paths (comma-separated)">
          <Input {...form.register('affected')} placeholder="src/foo.rs, src/bar.rs" />
        </Field>
      </FormShell>
    </form>
  )
}

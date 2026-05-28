import { useForm, type SubmitHandler } from 'react-hook-form'
import { zodResolver } from '@hookform/resolvers/zod'
import { Input } from '@/components/ui/input'
import { ScopeCombobox } from '@/components/ui/autocomplete'
import { MarkdownEditor } from '@/components/markdown-editor'
import { useCreateMemory } from '../use-memory-mutations'
import { gotchaSchema, type GotchaValues } from '../schemas'
import { Field, FormShell, RiskClassSelect } from './shared'

const FORM_ID = 'create-memory-gotcha'

export function GotchaForm({ onDone }: { onDone: () => void }) {
  const mutate = useCreateMemory()
  const form = useForm<GotchaValues>({
    resolver: zodResolver(gotchaSchema) as never,
    defaultValues: { summary: '', details: '', affected: '', scope: '' },
  })

  const onSubmit: SubmitHandler<GotchaValues> = async (v) => {
    const parsed = gotchaSchema.parse(v)
    await mutate.mutateAsync({
      kind: 'gotcha',
      summary: parsed.summary,
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
          <Input autoFocus {...form.register('summary')} placeholder="The trap, in one line" />
        </Field>
        <Field label="Details (Markdown)">
          <MarkdownEditor
            value={form.watch('details') ?? ''}
            onChange={(md) => form.setValue('details', md)}
            placeholder="Why it bit, how to avoid it"
          />
        </Field>
        <Field label="Affected paths (comma-separated)">
          <Input {...form.register('affected')} placeholder="src/foo.rs, src/bar.rs" />
        </Field>
        <div className="grid grid-cols-2 gap-3">
          <Field label="Risk class">
            <RiskClassSelect
              value={form.watch('riskClass')}
              onChange={(v) =>
                form.setValue('riskClass', v as GotchaValues['riskClass'])
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

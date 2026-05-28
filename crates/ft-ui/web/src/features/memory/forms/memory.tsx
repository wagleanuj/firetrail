import { useForm, type SubmitHandler } from 'react-hook-form'
import { zodResolver } from '@hookform/resolvers/zod'
import { Input } from '@/components/ui/input'
import { MarkdownEditor } from '@/components/markdown-editor'
import { useCreateMemory } from '../use-memory-mutations'
import { memorySchema, type MemoryValues } from '../schemas'
import { Field, FormShell, RiskClassSelect } from './shared'

const FORM_ID = 'create-memory-memory'

export function MemoryForm({ onDone }: { onDone: () => void }) {
  const mutate = useCreateMemory()
  const form = useForm<MemoryValues>({
    resolver: zodResolver(memorySchema) as never,
    defaultValues: { title: '', body: '', tags: '', scope: '' },
  })

  const onSubmit: SubmitHandler<MemoryValues> = async (v) => {
    const parsed = memorySchema.parse(v)
    await mutate.mutateAsync({
      kind: 'memory',
      title: parsed.title,
      body: parsed.body,
      tags: parsed.tags,
      riskClass: parsed.riskClass ?? null,
      scope: parsed.scope || null,
    })
    onDone()
  }

  return (
    <form id={FORM_ID} onSubmit={form.handleSubmit(onSubmit)}>
      <FormShell submitting={mutate.isPending} formId={FORM_ID}>
        <Field label="Title" error={form.formState.errors.title?.message}>
          <Input autoFocus {...form.register('title')} placeholder="Short, memorable" />
        </Field>
        <Field label="Body (Markdown)" error={form.formState.errors.body?.message}>
          <MarkdownEditor
            value={form.watch('body') ?? ''}
            onChange={(md) => form.setValue('body', md, { shouldValidate: true })}
            placeholder="Paste the note. Lasts forever (memory records are immutable)."
          />
        </Field>
        <Field label="Tags (comma-separated)">
          <Input {...form.register('tags')} placeholder="foo, bar, baz" />
        </Field>
        <div className="grid grid-cols-2 gap-3">
          <Field label="Risk class">
            <RiskClassSelect
              value={form.watch('riskClass')}
              onChange={(v) =>
                form.setValue('riskClass', v as MemoryValues['riskClass'])
              }
            />
          </Field>
          <Field label="Scope">
            <Input {...form.register('scope')} />
          </Field>
        </div>
      </FormShell>
    </form>
  )
}

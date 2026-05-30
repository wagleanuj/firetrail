import { useForm, type SubmitHandler } from 'react-hook-form'
import { zodResolver } from '@hookform/resolvers/zod'
import { Input } from '@/components/ui/input'
import { ScopeCombobox } from '@/components/ui/autocomplete'
import { MarkdownEditor } from '@/components/markdown-editor'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { useCreateMemory } from '../use-memory-mutations'
import { decisionSchema, DECISION_STATUSES, type DecisionValues } from '../schemas'
import { Field, FormShell, RiskClassSelect } from './shared'

const FORM_ID = 'create-memory-decision'

export function DecisionForm({ onDone }: { onDone: () => void }) {
  const mutate = useCreateMemory()
  const form = useForm<DecisionValues>({
    resolver: zodResolver(decisionSchema) as never,
    defaultValues: {
      title: '',
      context: '',
      decision: '',
      consequences: '',
      scope: '',
      alternatives: '',
    },
  })

  const onSubmit: SubmitHandler<DecisionValues> = async (v) => {
    const parsed = decisionSchema.parse(v)
    await mutate.mutateAsync({
      kind: 'decision',
      title: parsed.title,
      context: parsed.context,
      decision: parsed.decision,
      consequences: parsed.consequences || null,
      riskClass: parsed.riskClass ?? null,
      scope: parsed.scope || null,
      alternatives: parsed.alternatives,
      status: parsed.status ?? null,
    })
    onDone()
  }

  return (
    <form id={FORM_ID} onSubmit={form.handleSubmit(onSubmit)}>
      <FormShell submitting={mutate.isPending} formId={FORM_ID}>
        <Field label="Title" error={form.formState.errors.title?.message}>
          <Input autoFocus {...form.register('title')} placeholder="ADR-0042: Adopt foo" />
        </Field>
        <Field label="Context" error={form.formState.errors.context?.message}>
          <MarkdownEditor
            value={form.watch('context') ?? ''}
            onChange={(md) => form.setValue('context', md)}
            placeholder="Background / problem statement"
          />
        </Field>
        <Field label="Decision" error={form.formState.errors.decision?.message}>
          <MarkdownEditor
            value={form.watch('decision') ?? ''}
            onChange={(md) => form.setValue('decision', md)}
            placeholder="The decision itself"
          />
        </Field>
        <Field label="Consequences">
          <MarkdownEditor
            value={form.watch('consequences') ?? ''}
            onChange={(md) => form.setValue('consequences', md)}
            placeholder="Trade-offs / fallout"
          />
        </Field>
        <Field label="Alternatives considered (comma-separated)">
          <Input
            {...form.register('alternatives')}
            placeholder="Option B, Option C"
          />
        </Field>
        <div className="grid grid-cols-2 gap-3">
          <Field label="Status">
            <Select
              value={form.watch('status')}
              onValueChange={(v) =>
                form.setValue('status', (v as (typeof DECISION_STATUSES)[number]) || undefined)
              }
            >
              <SelectTrigger>
                <SelectValue placeholder="proposed" />
              </SelectTrigger>
              <SelectContent>
                {DECISION_STATUSES.map((s) => (
                  <SelectItem key={s} value={s}>
                    {s}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </Field>
          <Field label="Risk class">
            <RiskClassSelect
              value={form.watch('riskClass')}
              onChange={(v) =>
                form.setValue('riskClass', v as DecisionValues['riskClass'])
              }
            />
          </Field>
        </div>
        <Field label="Scope">
          <ScopeCombobox
            value={form.watch('scope') ?? ''}
            onValueChange={(v) => form.setValue('scope', v)}
          />
        </Field>
      </FormShell>
    </form>
  )
}

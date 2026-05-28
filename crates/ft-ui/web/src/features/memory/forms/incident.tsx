import { useForm, type SubmitHandler } from 'react-hook-form'
import { zodResolver } from '@hookform/resolvers/zod'
import { Input } from '@/components/ui/input'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { useCreateMemory } from '../use-memory-mutations'
import { incidentSchema, type IncidentValues } from '../schemas'
import { SEVERITIES } from '../types'
import { Field, FormShell, RiskClassSelect } from './shared'

const FORM_ID = 'create-memory-incident'

export function IncidentForm({ onDone }: { onDone: () => void }) {
  const mutate = useCreateMemory()
  const form = useForm<IncidentValues>({
    resolver: zodResolver(incidentSchema) as never,
    defaultValues: { summary: '', services: '', scope: '' },
  })

  const onSubmit: SubmitHandler<IncidentValues> = async (v) => {
    const parsed = incidentSchema.parse(v)
    await mutate.mutateAsync({
      kind: 'incident',
      summary: parsed.summary,
      severity: parsed.severity ?? null,
      startedAt: null,
      services: parsed.services,
      riskClass: parsed.riskClass ?? null,
      scope: parsed.scope || null,
    })
    onDone()
  }

  return (
    <form id={FORM_ID} onSubmit={form.handleSubmit(onSubmit)}>
      <FormShell submitting={mutate.isPending} formId={FORM_ID}>
        <Field label="Summary" error={form.formState.errors.summary?.message}>
          <Input autoFocus {...form.register('summary')} placeholder="What broke?" />
        </Field>
        <div className="grid grid-cols-2 gap-3">
          <Field label="Severity">
            <Select
              value={form.watch('severity')}
              onValueChange={(v) =>
                form.setValue('severity', (v as (typeof SEVERITIES)[number]) || undefined)
              }
            >
              <SelectTrigger>
                <SelectValue placeholder="Severity" />
              </SelectTrigger>
              <SelectContent>
                {SEVERITIES.map((s) => (
                  <SelectItem key={s} value={s}>
                    {s.toUpperCase()}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </Field>
          <Field label="Risk class">
            <RiskClassSelect
              value={form.watch('riskClass')}
              onChange={(v) =>
                form.setValue('riskClass', v as IncidentValues['riskClass'])
              }
            />
          </Field>
        </div>
        <Field label="Services (comma-separated)">
          <Input {...form.register('services')} placeholder="api, worker" />
        </Field>
        <Field label="Scope">
          <Input {...form.register('scope')} placeholder="scope id (optional)" />
        </Field>
      </FormShell>
    </form>
  )
}

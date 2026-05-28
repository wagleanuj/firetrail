/**
 * Identity- and scope-aware comboboxes.
 *
 * These are thin wrappers around the generic `<Combobox>` that load options
 * from the existing TanStack Query hooks. They expose the same value/onChange
 * shape as `<Input>` so they can drop into forms with a one-line swap.
 */
import { useIdentityList } from '@/features/identity/use-identity-query'
import { useScopeList } from '@/features/scope/use-scope-query'
import { Combobox, type ComboboxOption } from './combobox'

interface BaseProps {
  value: string
  onValueChange: (v: string) => void
  placeholder?: string
  className?: string
  'data-testid'?: string
}

export function OwnerCombobox(props: BaseProps) {
  const list = useIdentityList({ status: 'active' })
  const options: ComboboxOption[] =
    list.data?.identities.map((i) => ({
      value: i.id,
      label: i.id,
      detail: i.name || i.emails[0] || i.kind,
    })) ?? []
  return (
    <Combobox
      {...props}
      options={options}
      loading={list.isLoading}
      placeholder={props.placeholder ?? 'identity (optional)'}
    />
  )
}

export function ScopeCombobox(props: BaseProps) {
  const list = useScopeList()
  const options: ComboboxOption[] =
    list.data?.scopes.map((s) => ({
      value: s.id,
      label: s.id,
      detail: s.name,
    })) ?? []
  return (
    <Combobox
      {...props}
      options={options}
      loading={list.isLoading}
      placeholder={props.placeholder ?? 'scope id (optional)'}
    />
  )
}

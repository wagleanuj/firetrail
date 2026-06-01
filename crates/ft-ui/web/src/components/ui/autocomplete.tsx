/**
 * Identity- and scope-aware comboboxes.
 *
 * These are thin wrappers around the generic `<Combobox>` that load options
 * from the existing TanStack Query hooks. They expose the same value/onChange
 * shape as `<Input>` so they can drop into forms with a one-line swap.
 */
import { useEffect, useState } from 'react'
import { useIdentityList } from '@/features/identity/use-identity-query'
import { useScopeList } from '@/features/scope/use-scope-query'
import { useFiles } from '@/features/files/use-files-query'
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

interface FilePathProps extends BaseProps {
  /** Restrict suggestions to directory prefixes. */
  dirs?: boolean
}

/**
 * Path typeahead backed by `GET /api/files`. The typed value is debounced
 * (~180ms) into the prefix the query runs on, so each keystroke updates the
 * input immediately but the network/cache key only churns once typing pauses.
 *
 * Free-form values are allowed: the generic `<Combobox>` always renders the
 * current `value` (allowFreeform defaults to true), so a path that isn't in
 * the suggestion list still commits as-typed.
 */
export function FilePathCombobox({ dirs = false, ...props }: FilePathProps) {
  const [debounced, setDebounced] = useState(props.value)
  useEffect(() => {
    const handle = setTimeout(() => setDebounced(props.value), 180)
    return () => clearTimeout(handle)
  }, [props.value])

  const list = useFiles(debounced, dirs)
  const options: ComboboxOption[] =
    list.data?.paths.map((p) => ({ value: p, label: p })) ?? []
  return (
    <Combobox
      {...props}
      options={options}
      loading={list.isLoading}
      placeholder={props.placeholder ?? 'path (e.g. crates/ft-cli)'}
    />
  )
}

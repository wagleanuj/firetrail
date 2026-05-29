/**
 * Lightweight combobox built from an `Input` and a dropdown list.
 *
 * Why hand-rolled (no Radix Popover)? The Radix Popover primitive isn't on
 * this project's dependency list yet and the surface here is narrow enough
 * (single-select, freeform allowed, async-loaded options) to live without
 * the extra dep. Keyboard support: ↑/↓ to move highlight, ⏎ to commit,
 * Esc to close.
 */
import {
  forwardRef,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type InputHTMLAttributes,
  type KeyboardEvent,
} from 'react'
import { ChevronDown, Loader2, X } from 'lucide-react'
import { Input } from './input'
import { cn } from '@/lib/utils'

export interface ComboboxOption {
  /** Stable value sent on selection. */
  value: string
  /** What to render in the list (defaults to `value`). */
  label?: string
  /** Optional secondary line. */
  detail?: string
}

export interface ComboboxProps
  extends Omit<InputHTMLAttributes<HTMLInputElement>, 'onChange' | 'value'> {
  /** Current value. */
  value: string
  /** Called when the user picks an option or types freely. */
  onValueChange: (v: string) => void
  /** Async source. Called with the current query string. */
  options: ComboboxOption[]
  /** Whether the options query is loading. */
  loading?: boolean
  /** Placeholder for the underlying input. */
  placeholder?: string
  /** Allow free-form values that aren't in the options list. Defaults to true. */
  allowFreeform?: boolean
  /** Optional test id forwarded to the input. */
  'data-testid'?: string
}

export const Combobox = forwardRef<HTMLInputElement, ComboboxProps>(
  function Combobox(
    {
      value,
      onValueChange,
      options,
      loading = false,
      placeholder,
      allowFreeform = true,
      className,
      'data-testid': testId,
      ...rest
    },
    ref,
  ) {
    const [open, setOpen] = useState(false)
    const [highlight, setHighlight] = useState(0)
    const wrapperRef = useRef<HTMLDivElement>(null)

    // Close on outside click.
    useEffect(() => {
      if (!open) return
      function onDoc(e: MouseEvent) {
        if (!wrapperRef.current) return
        if (!wrapperRef.current.contains(e.target as Node)) {
          setOpen(false)
        }
      }
      document.addEventListener('mousedown', onDoc)
      return () => document.removeEventListener('mousedown', onDoc)
    }, [open])

    const lowered = value.trim().toLowerCase()
    const filtered = useMemo(() => {
      if (!lowered) return options
      return options.filter(
        (o) =>
          o.value.toLowerCase().includes(lowered) ||
          (o.label ?? '').toLowerCase().includes(lowered) ||
          (o.detail ?? '').toLowerCase().includes(lowered),
      )
    }, [options, lowered])

    const commit = useCallback(
      (v: string) => {
        onValueChange(v)
        setOpen(false)
      },
      [onValueChange],
    )

    function onKeyDown(e: KeyboardEvent<HTMLInputElement>) {
      if (e.key === 'ArrowDown') {
        e.preventDefault()
        setOpen(true)
        setHighlight((h) => Math.min(h + 1, Math.max(0, filtered.length - 1)))
      } else if (e.key === 'ArrowUp') {
        e.preventDefault()
        setHighlight((h) => Math.max(0, h - 1))
      } else if (e.key === 'Enter') {
        if (open && filtered.length > 0) {
          e.preventDefault()
          const picked = filtered[Math.min(highlight, filtered.length - 1)]
          commit(picked.value)
        } else if (allowFreeform) {
          setOpen(false)
        }
      } else if (e.key === 'Escape') {
        setOpen(false)
      }
    }

    return (
      <div ref={wrapperRef} className={cn('relative', className)}>
        <Input
          ref={ref}
          {...rest}
          data-testid={testId}
          value={value}
          placeholder={placeholder}
          onChange={(e) => {
            onValueChange(e.target.value)
            setOpen(true)
            setHighlight(0)
          }}
          onFocus={() => setOpen(true)}
          onKeyDown={onKeyDown}
          className="pr-8"
          autoComplete="off"
        />
        <div className="pointer-events-none absolute right-2 top-1/2 -translate-y-1/2 text-muted-foreground">
          {loading ? (
            <Loader2 className="h-3.5 w-3.5 animate-spin" />
          ) : value ? (
            <button
              type="button"
              onClick={() => commit('')}
              className="pointer-events-auto rounded hover:bg-muted/80"
              aria-label="Clear"
              tabIndex={-1}
            >
              <X className="h-3.5 w-3.5" />
            </button>
          ) : (
            <ChevronDown className="h-3.5 w-3.5" />
          )}
        </div>
        {open && (filtered.length > 0 || loading) && (
          <ul
            data-testid={testId ? `${testId}-list` : undefined}
            className="absolute z-50 mt-1 max-h-56 w-full overflow-y-auto rounded-md border border-border bg-surface-3 p-1 shadow-elevation-2"
          >
            {loading && filtered.length === 0 && (
              <li className="px-2 py-1.5 text-xs text-muted-foreground">
                Loading…
              </li>
            )}
            {filtered.map((opt, i) => (
              <li key={opt.value}>
                <button
                  type="button"
                  onMouseDown={(e) => {
                    e.preventDefault()
                    commit(opt.value)
                  }}
                  onMouseEnter={() => setHighlight(i)}
                  className={cn(
                    'flex w-full items-center justify-between gap-2 rounded px-2 py-1.5 text-left text-sm',
                    i === highlight ? 'bg-surface-2 text-accent-foreground' : '',
                  )}
                >
                  <span className="truncate font-mono text-xs">
                    {opt.label ?? opt.value}
                  </span>
                  {opt.detail && (
                    <span className="truncate text-[0.625rem] text-muted-foreground">
                      {opt.detail}
                    </span>
                  )}
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>
    )
  },
)

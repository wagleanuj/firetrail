/**
 * Search-mode segmented control (AUTO / LEXICAL / VECTOR / HYBRID).
 *
 * Shared by the dedicated memory search page and the Cmd+K command palette so
 * both surfaces expose the same modes and styling. `auto` is the smart default
 * — it runs hybrid (lexical + vector) when an embedding is available, falling
 * back to lexical otherwise.
 */
import type { SearchMode } from '@/api/types/SearchMode'
import { SEARCH_MODES } from '@/features/memory/types'
import { cn } from '@/lib/utils'

interface ModeSegmentedProps {
  value: SearchMode
  onChange: (mode: SearchMode) => void
  /** Optional size tweak — the palette uses a denser variant. */
  dense?: boolean
}

export function ModeSegmented({ value, onChange, dense = false }: ModeSegmentedProps) {
  return (
    <div
      role="radiogroup"
      aria-label="Search mode"
      className="inline-flex rounded-lg border border-border bg-surface-2 p-1"
    >
      {SEARCH_MODES.map((m) => (
        <button
          key={m}
          type="button"
          role="radio"
          aria-checked={m === value}
          data-testid={`mode-segment-${m}`}
          onClick={() => onChange(m)}
          className={cn(
            'rounded-md font-mono uppercase tracking-wider transition-colors',
            dense ? 'px-2 py-0.5 text-[0.625rem]' : 'px-3 py-1 text-xs',
            m === value
              ? 'bg-primary text-primary-foreground'
              : 'text-muted-foreground hover:text-foreground',
          )}
        >
          {m}
        </button>
      ))}
    </div>
  )
}

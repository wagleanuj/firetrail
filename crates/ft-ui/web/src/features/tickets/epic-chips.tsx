import type { BoardEpic } from '@/api/types/BoardEpic'
import { cn } from '@/lib/utils'
import { epicColor } from './epic-color'

const NO_EPIC = '' // sentinel id for "No epic"

export function EpicChips({
  epics,
  selected,
  onChange,
}: {
  epics: BoardEpic[]
  selected: Set<string>
  onChange: (next: Set<string>) => void
}) {
  function toggle(id: string) {
    const next = new Set(selected)
    if (next.has(id)) next.delete(id)
    else next.add(id)
    onChange(next)
  }

  const chip = (id: string, label: string, color?: string) => {
    const on = selected.has(id)
    return (
      <button
        key={id || 'no-epic'}
        type="button"
        aria-pressed={on}
        onClick={() => toggle(id)}
        className={cn(
          'rounded-full border px-2.5 py-0.5 text-xs transition-colors',
          on
            ? 'border-transparent text-foreground'
            : 'border-border/60 text-muted-foreground hover:text-foreground',
        )}
        style={on && color ? { background: `${color}22`, color } : undefined}
      >
        {label}
      </button>
    )
  }

  return (
    <div className="flex flex-wrap items-center gap-1.5">
      {epics.map((e) => chip(e.id, e.title, epicColor(e.id)))}
      {chip(NO_EPIC, 'No epic')}
    </div>
  )
}

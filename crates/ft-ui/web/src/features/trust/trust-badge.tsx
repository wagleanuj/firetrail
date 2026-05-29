/**
 * Trust-state color language — the shared visual vocabulary for memory trust.
 *
 * Per the ft-ui redesign spec (§7 Memory):
 *   - verified    → success  (vetted, load-bearing)
 *   - provisional → warning  (draft / reviewed / rejected — not yet trusted)
 *   - stale       → muted     by default, danger when staleness is emphasised
 *   - terminal    → muted    (deprecated / archived / superseded / redacted)
 *
 * `TrustBadge` is the single place every memory surface (list, search, detail)
 * renders a trust state, so the mapping stays consistent and scannable. It is
 * layout/visual only — it carries no behaviour and no API coupling.
 */
import { CheckCircle2, AlertTriangle, Clock, Archive, CircleDashed } from 'lucide-react'
import { cn } from '@/lib/utils'

/** Semantic tone applied to a trust state. */
type TrustTone = 'success' | 'warning' | 'muted' | 'danger'

interface TrustVisual {
  tone: TrustTone
  icon: React.ComponentType<{ className?: string }>
}

/**
 * Map every backend trust state to a tone + glyph. Unknown states fall back to
 * `muted` so a new server-side state degrades gracefully rather than throwing.
 */
const TRUST_VISUALS: Record<string, TrustVisual> = {
  verified: { tone: 'success', icon: CheckCircle2 },
  reviewed: { tone: 'warning', icon: AlertTriangle },
  draft: { tone: 'warning', icon: CircleDashed },
  rejected: { tone: 'warning', icon: AlertTriangle },
  stale: { tone: 'muted', icon: Clock },
  deprecated: { tone: 'muted', icon: Archive },
  archived: { tone: 'muted', icon: Archive },
  superseded: { tone: 'muted', icon: Archive },
  redacted: { tone: 'muted', icon: Archive },
}

function visualFor(state: string | null | undefined): TrustVisual {
  if (!state) return { tone: 'muted', icon: CircleDashed }
  return TRUST_VISUALS[state] ?? { tone: 'muted', icon: CircleDashed }
}

const TONE_CLASSES: Record<TrustTone, string> = {
  success: 'bg-success/15 text-success',
  warning: 'bg-warning/15 text-warning',
  danger: 'bg-danger/15 text-danger',
  muted: 'bg-muted text-muted-foreground',
}

/** Expose the tone resolution so dependent surfaces (e.g. detail panels) can
 *  reuse the same color decision without re-deriving it. */
export function trustTone(state: string | null | undefined): TrustTone {
  return visualFor(state).tone
}

interface TrustBadgeProps {
  state: string | null | undefined
  /** Emphasise staleness with the danger tone instead of the quiet muted one. */
  emphasizeStale?: boolean
  /** Hide the leading glyph for tight rows. */
  hideIcon?: boolean
  className?: string
}

export function TrustBadge({ state, emphasizeStale, hideIcon, className }: TrustBadgeProps) {
  const { tone, icon: Icon } = visualFor(state)
  const resolved: TrustTone = tone === 'muted' && emphasizeStale && state === 'stale' ? 'danger' : tone
  return (
    <span
      data-trust-state={state ?? 'unknown'}
      className={cn(
        'inline-flex items-center gap-1 rounded-full px-2 py-0.5 font-mono text-[0.625rem] font-semibold uppercase tracking-wider',
        TONE_CLASSES[resolved],
        className,
      )}
    >
      {!hideIcon && <Icon className="h-3 w-3" />}
      {state ?? 'n/a'}
    </span>
  )
}

/**
 * A 2px vertical rail expressing trust tone — used to flank list cards so the
 * trust state is scannable down a column without reading the badge.
 */
const RAIL_CLASSES: Record<TrustTone, string> = {
  success: 'bg-success',
  warning: 'bg-warning',
  danger: 'bg-danger',
  muted: 'bg-border',
}

export function trustRailClass(state: string | null | undefined, emphasizeStale = false): string {
  const tone = trustTone(state)
  const resolved: TrustTone = tone === 'muted' && emphasizeStale && state === 'stale' ? 'danger' : tone
  return RAIL_CLASSES[resolved]
}

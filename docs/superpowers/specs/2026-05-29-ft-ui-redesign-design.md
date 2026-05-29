# ft-ui Redesign — Design Spec

**Date:** 2026-05-29
**Direction:** D · Evolved (cyan DNA, elevated) — validated via visual companion
**Density:** Comfortable (borrows Editorial's breathing room)
**Scope:** Full redesign — tokens, shared components, app shell, all 5 domains
**Goals:** distinct identity · daily-use ergonomics · cohesion across domains · premium polish

This is the **contract** every implementation phase follows. Token values are
authoritative. Do not invent new colors/fonts outside this system.

---

## 1. Design tokens (authoritative)

All colors are HSL channels for CSS custom properties in `src/index.css`, consumed
by `tailwind.config.ts` as `hsl(var(--x))`. Dark is the primary, polished theme;
light is restrained but must stay legible.

### Dark theme (`.dark`, primary)

```
--background:        215 28% 7%     /* deep cool charcoal */
--foreground:        213 25% 92%
--card:              215 25% 10%
--card-foreground:   213 25% 92%
--popover:           215 25% 11%
--popover-foreground:213 25% 92%
--muted:             215 20% 15%
--muted-foreground:  215 14% 60%
--accent:            215 22% 16%
--accent-foreground: 213 25% 92%
--secondary:         215 22% 16%
--secondary-foreground:213 25% 92%
--primary:           187 92% 52%    /* cyan signal #22D3EE-ish */
--primary-foreground: 215 28% 7%
--border:            215 18% 18%
--input:             215 18% 18%
--ring:              187 92% 52%
--destructive:       0 72% 58%
--destructive-foreground: 213 25% 95%

/* semantic */
--success:           152 60% 50%
--warning:           38 92% 58%
--danger:            0 75% 64%
--info:              187 92% 52%

/* elevation surfaces (new — for depth) */
--surface-1:         215 25% 10%    /* = card */
--surface-2:         215 24% 13%    /* raised: popovers, hovered cards */
--surface-3:         215 22% 16%    /* overlays, command palette */

/* type-of-work accents (chips/pills) */
--type-feature:      187 92% 52%    /* cyan */
--type-bug:          0 75% 64%      /* red */
--type-task:         255 92% 78%    /* violet */
--type-epic:         38 92% 60%     /* amber */
```

### Light theme (`:root`, restrained)

```
--background: 0 0% 100%;  --foreground: 215 28% 12%
--card: 0 0% 100%;        --card-foreground: 215 28% 12%
--popover: 0 0% 100%;     --popover-foreground: 215 28% 12%
--muted: 215 20% 96%;     --muted-foreground: 215 12% 42%
--accent: 215 20% 95%;    --accent-foreground: 215 28% 12%
--secondary: 215 20% 95%; --secondary-foreground: 215 28% 12%
--primary: 191 91% 38%;   --primary-foreground: 0 0% 100%
--border: 215 16% 88%;    --input: 215 16% 88%;  --ring: 191 91% 42%
--destructive: 0 72% 51%; --destructive-foreground: 0 0% 100%
--success: 152 55% 40%;   --warning: 38 90% 45%;  --danger: 0 72% 55%; --info: 191 91% 40%
--surface-1: 0 0% 100%;   --surface-2: 215 20% 98%; --surface-3: 215 20% 96%
--type-feature: 191 91% 40%; --type-bug: 0 72% 55%; --type-task: 255 70% 60%; --type-epic: 38 90% 42%
--radius: 0.625rem
```

`--radius` stays `0.625rem` (10px).

### Tailwind config additions (`tailwind.config.ts`)

- Add color entries: `success`, `warning`, `danger`, `info` (already partial), plus
  `surface: { 1, 2, 3 }` and `type: { feature, bug, task, epic }`, all `hsl(var(--x))`.
- Keep existing `border/input/ring/background/foreground/primary/secondary/destructive/muted/accent/popover/card`.
- `boxShadow` extension for depth:
  - `elevation-1: '0 1px 2px 0 hsl(215 40% 3% / 0.4)'`
  - `elevation-2: '0 4px 16px -2px hsl(215 40% 3% / 0.5)'`
  - `glow: '0 0 16px hsl(var(--primary) / 0.45)'` (logo dot, focused/active accents — use sparingly)

---

## 2. Typography

- **Display / headings / wordmark:** `Sora` (600/700). Add to `fontFamily.display`.
- **Body / UI:** `Inter` (existing `sans`). Keep `font-feature-settings: 'cv02','cv03','cv04','cv11'`.
- **Mono / ids / code:** `JetBrains Mono` (existing `mono`). Issue ids (`ft-44v`),
  counts, code blocks.
- **Load fonts locally** via `@fontsource` (preferred — no network dependency at
  runtime; the app is served from a local Axum binary). Add `@fontsource/sora`,
  `@fontsource-variable/inter` (or @fontsource/inter), `@fontsource/jetbrains-mono`
  and import in `src/main.tsx`. If adding deps is undesirable, fall back to a single
  `@import` in `index.css`, but local is the target.

Type scale (Tailwind defaults are fine; apply consistently):
- Page title: `text-xl font-display font-semibold tracking-tight`
- Section heading: `text-sm font-medium uppercase tracking-wide text-muted-foreground`
- Card title: `text-sm font-medium`
- Meta / ids: `text-xs font-mono text-muted-foreground`

---

## 3. Spacing & density (Comfortable)

- Card padding: `p-3` (12px). Card gap in columns: `gap-2.5` (10px).
- Page gutter: `px-6`. Content max width for centered views: `max-w-6xl`.
- Comfortable line-height on titles (`leading-snug`). One card = one clear unit.
- Active/in-progress card gets a soft cyan ring: `ring-1 ring-primary/25` + `shadow-glow` at low opacity.

---

## 4. Motion language

Extend `src/lib/motion.ts` (framer-motion). Principles: quick, subtle, never blocking.
- Route transitions: fade + 4px rise, 160ms ease-out.
- Card enter/reorder: layout animation via dnd-kit + framer `layout`, 140ms.
- Hover: `transition-colors`/`transition-transform` 120ms.
- Respect `prefers-reduced-motion` — gate non-essential motion.

---

## 5. Shared component restyle (Phase 0)

Restyle in place (keep APIs/props identical — these are consumed everywhere):
`button, input, textarea, select, badge, card, dialog, sheet, dropdown-menu,
alert-dialog, tabs, tooltip, table, scroll-area, separator, label, skeleton,
empty-state, error-boundary, sonner, combobox, autocomplete`.

- **Button:** primary = cyan fill; ghost/secondary use `surface-2`; focus-visible ring.
- **Badge:** add `feature|bug|task|epic` variants using `type-*` tokens (subtle tinted bg + colored text, matches the mockup pills).
- **Card:** `bg-card border-border` + `hover:bg-surface-2 transition-colors`; optional `shadow-elevation-1`.
- **Dialog/Sheet/Dropdown/Command:** `bg-surface-3` + `shadow-elevation-2` + `border-border`.
- **Skeleton:** shimmer using `surface-2 → surface-3`.
- Do **not** change component prop signatures. Visual-only.

---

## 6. App shell & navigation (Phase 1)

Replace top-nav (`app-shell.tsx`) with:
- **Left sidebar** (collapsible, ~220px): wordmark + glowing cyan dot at top; nav
  items Board / Memory / Scope / Identity / Audit with lucide icons + labels;
  active item = `bg-primary/10 text-primary` + left accent bar. Collapses to icon-rail.
- **Shared `PageHeader` component** (`components/page-header.tsx`): title (font-display),
  optional subtitle, right-aligned action slot, optional tabs row. Every domain route
  renders through it → cohesion.
- **Command palette** (`components/command-palette.tsx`): Cmd+K. Built on existing
  Radix primitives + a lightweight list (no new heavy dep; can use `cmdk` only if
  already feasible — otherwise compose with existing dialog + input + keyboard nav).
  Actions: navigate to each domain, create ticket, search memory, jump to ticket by id.
  Wire into existing `ShortcutsProvider`.
- Keep `RouteTransition`; align timing with §4.
- Preserve all existing SSE event-subscription hooks in the shell.

---

## 7. Per-domain visual systems (Phases 2–6)

Each domain reworks its screens to use the new shell `PageHeader`, restyled components,
and tokens. Keep all data/query/mutation/event logic intact — visual + layout only.

- **Board (tickets):** Comfortable cards per §3; column headers per mockup; type pills;
  active-card cyan ring; optimistic-feel drag polish; skeleton columns on load.
- **Memory:** trust-state color language — `verified→success`, `provisional→warning`,
  `stale→muted/danger`. List + search + detail consistent; salvage queue uses same.
- **Scope:** boundary cards/list using shared patterns; detail uses PageHeader + tabs.
- **Identity:** actor cards with avatar chips (reuse board avatar style); detail view.
- **Audit:** diff viewer + lineage graph restyled to token palette (graph node/edge
  colors from `--primary`/`--border`/semantic); verify/criteria/review screens cohesive.

Each domain phase must keep existing tests green and not alter prop/route contracts.

---

## 8. Phasing, dependencies, ship-safety

```
Phase 0 Foundation ──▶ Phase 1 Shell ──▶ ┌─ Phase 2 Board
                                          ├─ Phase 3 Memory
(tokens + components)  (sidebar/header/   ├─ Phase 4 Scope
                        command palette)  ├─ Phase 5 Identity
                                          └─ Phase 6 Audit   (parallelizable)
```

- App must build (`pnpm build`) and stay shippable after **every** phase.
- Phase 0 alone visibly refreshes the whole app (all screens consume tokens).
- Domains (2–6) are file-disjoint (each in `features/<d>/` + `routes/<d>/`) → safe to
  parallelize once 0 & 1 land.

## 9. Verification gate (every phase)

```
pnpm --dir crates/ft-ui/web typecheck
pnpm --dir crates/ft-ui/web lint
pnpm --dir crates/ft-ui/web test
pnpm --dir crates/ft-ui/web build
```

All must pass before a phase is considered done. Existing tests must not regress.
No prop/route/API contract changes — visual and layout only.

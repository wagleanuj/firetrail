import { createFileRoute } from '@tanstack/react-router'

export const Route = createFileRoute('/')({
  component: HomePage,
})

function HomePage() {
  return (
    <section className="mx-auto max-w-3xl px-6 py-16">
      <div className="rounded-xl border border-border/60 bg-card/50 p-8 shadow-[0_0_0_1px_hsl(var(--border)/0.4)_inset]">
        <div className="mb-4 inline-flex items-center gap-2 rounded-full border border-primary/30 bg-primary/10 px-3 py-1 text-xs font-medium uppercase tracking-wider text-primary">
          <span className="inline-block h-1.5 w-1.5 rounded-full bg-primary" />
          Wave 0 ready
        </div>
        <h1 className="font-mono text-4xl font-semibold tracking-tight">Firetrail</h1>
        <p className="mt-3 max-w-prose text-muted-foreground">
          Local-first knowledge GUI for trust-aware tickets, scope-bounded memory, and verifiable
          PR provenance. The scaffold is up — Waves 1–3 will wire in the ops surface.
        </p>
        <div className="mt-6 grid grid-cols-1 gap-3 text-sm sm:grid-cols-3">
          <Stat label="Backend" value="ft-ui (axum)" />
          <Stat label="Transport" value="REST + SSE" />
          <Stat label="State" value="TanStack Query" />
        </div>
      </div>
    </section>
  )
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-lg border border-border/60 bg-background/40 px-4 py-3">
      <div className="text-xs uppercase tracking-wider text-muted-foreground">{label}</div>
      <div className="mt-1 font-mono text-sm">{value}</div>
    </div>
  )
}

/**
 * Audit dashboard. Four cards, one per audit op. Click → deep-link to the
 * dedicated sub-route. Each card has just enough state (no global store) to
 * remember last-run summary while the user is on the page.
 */
import { Link } from '@tanstack/react-router'
import { ShieldAlert, ShieldCheck, GitCompare, Network, History } from 'lucide-react'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { EmptyState } from '@/components/ui/empty-state'
import { PageHeader } from '@/components/page-header'

const TILES = [
  {
    to: '/audit/lint',
    icon: ShieldAlert,
    title: 'Lint',
    description: 'Scan every record for rule violations and surface suggested fixes.',
  },
  {
    to: '/audit/verify',
    icon: ShieldCheck,
    title: 'Verify',
    description: 'Walk each record\'s history chain and confirm signatures hold.',
  },
  {
    to: '/audit/diff',
    icon: GitCompare,
    title: 'Diff',
    description: 'Compare two refs and classify per-record changes.',
  },
  {
    to: '/audit/graph',
    icon: Network,
    title: 'Graph',
    description: 'Walk relations outward from a record. Force-directed view.',
  },
] as const

export function AuditDashboard() {
  return (
    <div className="mx-auto max-w-6xl space-y-6 px-6 py-6">
      <PageHeader
        title="Audit"
        subtitle="Lint, verify, diff, and walk the relation graph."
      />
      <section className="space-y-2.5">
        <h2 className="text-sm font-medium uppercase tracking-wide text-muted-foreground">
          Recent runs
        </h2>
        <EmptyState
          icon={History}
          title="No recent runs"
          description="Lint, verify, and diff runs from this session will appear here once you launch one."
        />
      </section>
      <div className="grid grid-cols-1 gap-2.5 md:grid-cols-2">
        {TILES.map((tile) => {
          const Icon = tile.icon
          return (
            <Link
              key={tile.to}
              to={tile.to}
              className="group block rounded-[var(--radius)] focus:outline-none focus-visible:ring-1 focus-visible:ring-ring"
              data-testid={`audit-tile-${tile.title.toLowerCase()}`}
            >
              <Card className="h-full p-3 transition-all hover:bg-surface-2 group-hover:-translate-y-0.5 group-hover:border-primary/40 group-focus-visible:border-primary/60">
                <CardHeader className="p-0">
                  <div className="flex items-center gap-3">
                    <span className="rounded-md bg-primary/15 p-2 text-primary">
                      <Icon className="h-4 w-4" />
                    </span>
                    <CardTitle className="text-sm font-medium">{tile.title}</CardTitle>
                  </div>
                </CardHeader>
                <CardContent className="p-0 pt-2">
                  <CardDescription className="leading-snug">{tile.description}</CardDescription>
                </CardContent>
              </Card>
            </Link>
          )
        })}
      </div>
    </div>
  )
}

/**
 * Audit dashboard. Four cards, one per audit op. Click → deep-link to the
 * dedicated sub-route. Each card has just enough state (no global store) to
 * remember last-run summary while the user is on the page.
 */
import { Link } from '@tanstack/react-router'
import { ShieldAlert, ShieldCheck, GitCompare, Network } from 'lucide-react'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'

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
    <div className="mx-auto max-w-6xl space-y-4 p-6">
      <header>
        <h1 className="font-mono text-lg font-semibold tracking-tight">Audit</h1>
        <p className="text-sm text-muted-foreground">
          Lint, verify, diff, and walk the relation graph.
        </p>
      </header>
      <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
        {TILES.map((tile) => {
          const Icon = tile.icon
          return (
            <Link
              key={tile.to}
              to={tile.to}
              className="group block focus:outline-none"
              data-testid={`audit-tile-${tile.title.toLowerCase()}`}
            >
              <Card className="h-full transition-all group-hover:-translate-y-0.5 group-hover:border-primary/40 group-focus-visible:border-primary/60">
                <CardHeader>
                  <div className="flex items-center gap-3">
                    <span className="rounded-md bg-primary/15 p-2 text-primary">
                      <Icon className="h-4 w-4" />
                    </span>
                    <CardTitle className="font-mono text-base">{tile.title}</CardTitle>
                  </div>
                </CardHeader>
                <CardContent>
                  <CardDescription>{tile.description}</CardDescription>
                </CardContent>
              </Card>
            </Link>
          )
        })}
      </div>
    </div>
  )
}

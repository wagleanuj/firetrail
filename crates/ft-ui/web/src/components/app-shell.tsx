import { Link, Outlet } from '@tanstack/react-router'
import { cn } from '@/lib/utils'
import { useTicketEvents } from '@/features/tickets/use-ticket-events'

const NAV = [
  { to: '/', label: 'Board' },
] as const

export function AppShell() {
  // Subscribe once at the shell level so the SSE connection survives navigation
  // between `/` and `/tickets/:id`.
  useTicketEvents()
  return (
    <div className="flex h-full flex-col">
      <header className="border-b border-border/60 bg-card/40 backdrop-blur">
        <div className="mx-auto flex h-14 max-w-6xl items-center gap-6 px-6">
          <Link to="/" className="flex items-center gap-2">
            <span className="inline-block h-2.5 w-2.5 rounded-full bg-primary shadow-[0_0_12px_hsl(var(--primary)/0.7)]" />
            <span className="font-mono text-sm font-semibold tracking-tight">firetrail</span>
          </Link>
          <nav className="flex items-center gap-1 text-sm text-muted-foreground">
            {NAV.map((item) => (
              <Link
                key={item.to}
                to={item.to}
                activeProps={{ className: 'text-foreground' }}
                className={cn(
                  'rounded-md px-3 py-1.5 transition-colors hover:text-foreground',
                )}
              >
                {item.label}
              </Link>
            ))}
          </nav>
        </div>
      </header>
      <main className="flex-1 overflow-y-auto">
        <Outlet />
      </main>
    </div>
  )
}

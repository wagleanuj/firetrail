import { Link } from '@tanstack/react-router'
import { RouteTransition } from '@/components/route-transition'
import { ShortcutsProvider } from '@/components/shortcuts'
import { Button } from '@/components/ui/button'
import { Keyboard } from 'lucide-react'
import { cn } from '@/lib/utils'
import { useTicketEvents } from '@/features/tickets/use-ticket-events'
import { useMemoryEvents } from '@/features/memory/use-memory-events'
import { useScopeEvents } from '@/features/scope/use-scope-events'
import { useIdentityEvents } from '@/features/identity/use-identity-events'
import { useAuditEvents } from '@/features/audit/use-audit-events'

const NAV = [
  { to: '/', label: 'Board' },
  { to: '/memory', label: 'Memory' },
  { to: '/scope', label: 'Scope' },
  { to: '/identity', label: 'Identity' },
  { to: '/audit', label: 'Audit' },
] as const

export function AppShell() {
  // Subscribe once at the shell level so the SSE connection survives
  // navigation between routes. Each consumer filters by event-kind prefix.
  useTicketEvents()
  useMemoryEvents()
  useScopeEvents()
  useIdentityEvents()
  useAuditEvents()
  return (
    <ShortcutsProvider>
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
                  activeProps={{ className: 'text-primary bg-primary/10' }}
                  activeOptions={{ exact: item.to === '/' }}
                  className={cn(
                    'rounded-md px-3 py-1.5 transition-colors hover:text-foreground',
                  )}
                >
                  {item.label}
                </Link>
              ))}
            </nav>
            <Button
              size="icon"
              variant="ghost"
              className="ml-auto h-8 w-8"
              aria-label="Show keyboard shortcuts"
              title="Keyboard shortcuts (?)"
              onClick={() => {
                // Dispatch the shift+/ shortcut programmatically via a fake event.
                window.dispatchEvent(
                  new KeyboardEvent('keydown', { key: '?', shiftKey: true, bubbles: true }),
                )
              }}
            >
              <Keyboard className="h-4 w-4" />
            </Button>
          </div>
        </header>
        <main className="flex-1 overflow-y-auto">
          <RouteTransition />
        </main>
      </div>
    </ShortcutsProvider>
  )
}

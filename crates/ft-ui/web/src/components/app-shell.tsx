import { RouteTransition } from '@/components/route-transition'
import { ShortcutsProvider } from '@/components/shortcuts'
import { Sidebar } from '@/components/sidebar'
import { useTicketEvents } from '@/features/tickets/use-ticket-events'
import { useMemoryEvents } from '@/features/memory/use-memory-events'
import { useScopeEvents } from '@/features/scope/use-scope-events'
import { useIdentityEvents } from '@/features/identity/use-identity-events'
import { useAuditEvents } from '@/features/audit/use-audit-events'

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
      <div className="flex h-full">
        <Sidebar />
        <main className="min-w-0 flex-1 overflow-y-auto">
          <RouteTransition />
        </main>
      </div>
    </ShortcutsProvider>
  )
}

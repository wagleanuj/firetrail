import { createFileRoute, useNavigate } from '@tanstack/react-router'
import { Sheet, SheetContent, SheetTitle, SheetDescription } from '@/components/ui/sheet'
import { TicketDetail } from '@/features/tickets/ticket-detail'
import { FeatureErrorBoundary } from '@/components/ui/error-boundary'

/**
 * `/tickets/:id` — the ticket detail drawer. Lives under the `_board` layout
 * so it slides in over a persistent board instead of remounting it. Closing
 * the drawer navigates back to `/`, preserving the board's search filters.
 */
export const Route = createFileRoute('/_board/tickets/$id')({
  component: TicketDrawer,
})

function TicketDrawer() {
  const { id } = Route.useParams()
  const navigate = useNavigate()
  return (
    <Sheet
      open
      onOpenChange={(o) => {
        if (!o) void navigate({ to: '/', search: (prev) => prev })
      }}
    >
      <SheetContent side="right" className="sm:max-w-2xl">
        <SheetTitle className="sr-only">Ticket {id}</SheetTitle>
        <SheetDescription className="sr-only">
          Ticket details and actions.
        </SheetDescription>
        <FeatureErrorBoundary>
          <TicketDetail id={id} />
        </FeatureErrorBoundary>
      </SheetContent>
    </Sheet>
  )
}

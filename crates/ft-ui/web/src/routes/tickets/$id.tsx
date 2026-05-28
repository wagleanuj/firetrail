import { useState } from 'react'
import { createFileRoute, useNavigate } from '@tanstack/react-router'
import { Sheet, SheetContent } from '@/components/ui/sheet'
import { Board } from '@/features/tickets/board'
import { CreateDialog } from '@/features/tickets/create-dialog'
import { TicketDetail } from '@/features/tickets/ticket-detail'
import { FeatureErrorBoundary } from '@/components/ui/error-boundary'

/**
 * `/tickets/:id` — kanban board with the ticket drawer overlaid.
 *
 * We render the board *and* the sheet here so deep-linking to a ticket still
 * shows the context behind it. Closing the sheet navigates back to `/`.
 */
export const Route = createFileRoute('/tickets/$id')({
  component: TicketRoute,
})

function TicketRoute() {
  const { id } = Route.useParams()
  const navigate = useNavigate()
  const [createOpen, setCreateOpen] = useState(false)
  return (
    <FeatureErrorBoundary>
      <Board onCreateClick={() => setCreateOpen(true)} />
      <CreateDialog open={createOpen} onOpenChange={setCreateOpen} />
      <Sheet
        open
        onOpenChange={(o) => {
          if (!o) navigate({ to: '/' })
        }}
      >
        <SheetContent side="right" className="sm:max-w-2xl">
          <FeatureErrorBoundary>
            <TicketDetail id={id} />
          </FeatureErrorBoundary>
        </SheetContent>
      </Sheet>
    </FeatureErrorBoundary>
  )
}

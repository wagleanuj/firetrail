import { useMemo, useState } from 'react'
import { createFileRoute, useNavigate, Outlet } from '@tanstack/react-router'
import { z } from 'zod'
import { Board } from '@/features/tickets/board'
import { CreateDialog } from '@/features/tickets/create-dialog'
import { FeatureErrorBoundary } from '@/components/ui/error-boundary'
import { useRegisterShortcut } from '@/components/shortcuts'

const searchSchema = z.object({
  ready: z.boolean().optional(),
  /** Epic ids to filter the board to. Lets the Epics view deep-link here. */
  epics: z.array(z.string()).optional(),
})

/**
 * Pathless layout shared by the board (`/`) and the ticket drawer
 * (`/tickets/:id`). The `<Board>` lives here so it stays mounted while the
 * drawer opens and closes over it — opening a ticket renders only the child
 * route's `<Sheet>` into `<Outlet>`, the board behind it never remounts.
 */
export const Route = createFileRoute('/_board')({
  validateSearch: searchSchema,
  component: BoardLayout,
})

function BoardLayout() {
  const search = Route.useSearch()
  const navigate = useNavigate({ from: '/' })
  const [createOpen, setCreateOpen] = useState(false)
  useRegisterShortcut({ openTicketCreate: () => setCreateOpen(true) })

  const epicFilter = useMemo(() => new Set(search.epics ?? []), [search.epics])

  return (
    <FeatureErrorBoundary>
      <Board
        onCreateClick={() => setCreateOpen(true)}
        ready={search.ready ?? false}
        onReadyChange={(v) => {
          void navigate({
            search: (prev) => ({ ...prev, ready: v ? true : undefined }),
            replace: true,
          })
        }}
        epicFilter={epicFilter}
        onEpicFilterChange={(next) => {
          void navigate({
            search: (prev) => ({ ...prev, epics: next.size ? Array.from(next) : undefined }),
            replace: true,
          })
        }}
      />
      <CreateDialog open={createOpen} onOpenChange={setCreateOpen} />
      {/* Child routes (the ticket drawer) render on top of the board. */}
      <Outlet />
    </FeatureErrorBoundary>
  )
}

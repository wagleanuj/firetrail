import { useMemo, useState } from 'react'
import { createFileRoute, useNavigate } from '@tanstack/react-router'
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

export const Route = createFileRoute('/')({
  validateSearch: searchSchema,
  component: HomePage,
})

function HomePage() {
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
    </FeatureErrorBoundary>
  )
}

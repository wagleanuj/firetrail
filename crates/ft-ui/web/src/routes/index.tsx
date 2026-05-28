import { useState } from 'react'
import { createFileRoute, useNavigate } from '@tanstack/react-router'
import { z } from 'zod'
import { Board } from '@/features/tickets/board'
import { CreateDialog } from '@/features/tickets/create-dialog'
import { FeatureErrorBoundary } from '@/components/ui/error-boundary'
import { useRegisterShortcut } from '@/components/shortcuts'

const searchSchema = z.object({
  ready: z.boolean().optional(),
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
      />
      <CreateDialog open={createOpen} onOpenChange={setCreateOpen} />
    </FeatureErrorBoundary>
  )
}

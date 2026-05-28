import { useState } from 'react'
import { createFileRoute } from '@tanstack/react-router'
import { Board } from '@/features/tickets/board'
import { CreateDialog } from '@/features/tickets/create-dialog'

export const Route = createFileRoute('/')({
  component: HomePage,
})

function HomePage() {
  const [createOpen, setCreateOpen] = useState(false)
  return (
    <>
      <Board onCreateClick={() => setCreateOpen(true)} />
      <CreateDialog open={createOpen} onOpenChange={setCreateOpen} />
    </>
  )
}

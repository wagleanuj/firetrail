import { createFileRoute } from '@tanstack/react-router'

/**
 * `/` — the board itself is rendered by the `_board` layout. The index route
 * has no drawer to overlay, so it renders nothing into the layout `<Outlet>`.
 */
export const Route = createFileRoute('/_board/')({
  component: () => null,
})

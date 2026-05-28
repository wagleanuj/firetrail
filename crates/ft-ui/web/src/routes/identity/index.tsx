import { createFileRoute } from '@tanstack/react-router'
import { IdentityPanel } from '@/features/identity/identity-panel'

export const Route = createFileRoute('/identity/')({
  component: () => <IdentityPanel />,
})

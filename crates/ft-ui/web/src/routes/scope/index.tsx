import { createFileRoute } from '@tanstack/react-router'
import { ScopeExplorer } from '@/features/scope/scope-explorer'

export const Route = createFileRoute('/scope/')({
  component: () => <ScopeExplorer />,
})

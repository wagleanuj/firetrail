/**
 * Feature-level React error boundary. Catches render-time exceptions thrown
 * inside a route's main component, renders a fallback panel with the error
 * message, and offers a "Try again" button that resets both the boundary and
 * any TanStack Query failures in scope.
 *
 * Errors thrown during `useQuery` are NOT caught here unless the query has
 * `throwOnError: true` — but `useQueryErrorResetBoundary` still resets stale
 * failure state so the next mount retries.
 */
import * as React from 'react'
import { AlertTriangle, RotateCcw } from 'lucide-react'
import { useQueryErrorResetBoundary } from '@tanstack/react-query'
import { Button } from '@/components/ui/button'
import { humanizeError } from '@/lib/errors'

interface FallbackProps {
  error: Error
  reset: () => void
}

function DefaultFallback({ error, reset }: FallbackProps) {
  const human = humanizeError(error)
  return (
    <div
      role="alert"
      className="mx-auto my-8 flex max-w-lg flex-col gap-3 rounded-md border border-destructive/40 bg-destructive/5 p-4 text-sm"
    >
      <div className="flex items-center gap-2 font-mono text-destructive">
        <AlertTriangle className="h-4 w-4" />
        <span className="font-semibold">{human.title}</span>
      </div>
      {human.description && <p className="text-muted-foreground">{human.description}</p>}
      <div>
        <Button size="sm" variant="outline" onClick={reset} className="gap-2">
          <RotateCcw className="h-3.5 w-3.5" />
          Try again
        </Button>
      </div>
    </div>
  )
}

interface BoundaryProps {
  children: React.ReactNode
  fallback?: (props: FallbackProps) => React.ReactNode
  onReset?: () => void
}

interface BoundaryState {
  error: Error | null
}

class ErrorBoundary extends React.Component<BoundaryProps, BoundaryState> {
  state: BoundaryState = { error: null }

  static getDerivedStateFromError(error: Error): BoundaryState {
    return { error }
  }

  componentDidCatch(error: Error, info: React.ErrorInfo): void {
    // Best-effort log so the error reaches the browser devtools console even
    // though we render a friendly fallback above.
    // eslint-disable-next-line no-console
    console.error('[FeatureErrorBoundary]', error, info)
  }

  reset = (): void => {
    this.props.onReset?.()
    this.setState({ error: null })
  }

  render(): React.ReactNode {
    if (this.state.error) {
      const Fallback = this.props.fallback ?? DefaultFallback
      return <Fallback error={this.state.error} reset={this.reset} />
    }
    return this.props.children
  }
}

/**
 * Wraps a feature route's main component in an error boundary that also
 * clears TanStack Query failures on reset, so a transient backend error can
 * be retried without a full page reload.
 */
export function FeatureErrorBoundary({
  children,
  fallback,
}: {
  children: React.ReactNode
  fallback?: (props: FallbackProps) => React.ReactNode
}) {
  const { reset } = useQueryErrorResetBoundary()
  return (
    <ErrorBoundary onReset={reset} fallback={fallback}>
      {children}
    </ErrorBoundary>
  )
}

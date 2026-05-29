/**
 * Capability matrix — table of {capability, granted, overridden} per identity.
 *
 * The "overridden" column shows a gear icon when the value differs from the
 * kind default. Hover surfaces a tooltip explaining the source.
 */
import { Check, X, Settings2 } from 'lucide-react'
import { Skeleton } from '@/components/ui/skeleton'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@/components/ui/tooltip'
import { cn } from '@/lib/utils'
import { useCapabilities } from './use-identity-query'

export function CapabilityMatrix({ identity }: { identity: string }) {
  const { data, isLoading, error } = useCapabilities(identity)
  if (isLoading) return <Skeleton className="h-48 w-full rounded-lg" />
  if (error) {
    return (
      <p className="text-sm text-destructive">
        Failed to load capabilities: {(error as Error).message}
      </p>
    )
  }
  if (!data) return null
  if (data.capabilities.length === 0) {
    return (
      <p className="rounded-lg border border-dashed border-border px-3 py-4 text-sm text-muted-foreground">
        No capabilities resolved for {data.identity}.
      </p>
    )
  }
  return (
    <TooltipProvider delayDuration={150}>
      <div className="overflow-hidden rounded-lg border border-border bg-card shadow-elevation-1">
      <Table data-testid="capability-matrix">
        <TableHeader>
          <TableRow>
            <TableHead>Capability</TableHead>
            <TableHead className="w-20 text-center">Granted</TableHead>
            <TableHead className="w-24 text-center">Source</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {data.capabilities.map((row) => (
            <TableRow key={row.capability}>
              <TableCell>
                <code className="font-mono text-xs">{row.capability}</code>
              </TableCell>
              <TableCell className="text-center">
                {row.granted ? (
                  <Check
                    className={cn('mx-auto h-4 w-4 text-primary')}
                    aria-label="granted"
                  />
                ) : (
                  <X
                    className="mx-auto h-4 w-4 text-muted-foreground"
                    aria-label="denied"
                  />
                )}
              </TableCell>
              <TableCell className="text-center">
                {row.overridden ? (
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <span
                        className="inline-flex items-center gap-1 font-mono text-[0.625rem] uppercase tracking-wider text-primary"
                        data-testid={`override-${row.capability}`}
                      >
                        <Settings2 className="h-3 w-3" />
                        override
                      </span>
                    </TooltipTrigger>
                    <TooltipContent>
                      Set explicitly on this identity. Overrides the{' '}
                      <code className="font-mono text-xs">{data.kind}</code>{' '}
                      kind default.
                    </TooltipContent>
                  </Tooltip>
                ) : (
                  <span className="font-mono text-[0.625rem] uppercase tracking-wider text-muted-foreground">
                    default
                  </span>
                )}
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
      </div>
    </TooltipProvider>
  )
}

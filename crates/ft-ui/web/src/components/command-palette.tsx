/**
 * Global command palette — opened with Cmd/Ctrl+K (wired in
 * `ShortcutsProvider`). Built on `cmdk`, which composes a Radix Dialog under
 * the hood, so focus-trapping and Esc-to-close come for free.
 *
 * Actions:
 *   - navigate to each of the five domains (Board / Memory / Scope /
 *     Identity / Audit)
 *   - create a ticket (routes home with the create flag)
 *   - search memory (routes to /memory and focuses its search input)
 *
 * Styling follows §5/§6 of the redesign spec: surface-3 with elevation-2.
 */
import * as React from 'react'
import { Command } from 'cmdk'
import * as DialogPrimitive from '@radix-ui/react-dialog'
import { useNavigate } from '@tanstack/react-router'
import {
  KanbanSquare,
  Brain,
  Boxes,
  Users,
  ScrollText,
  Plus,
  Search,
} from 'lucide-react'
import { cn } from '@/lib/utils'

interface CommandPaletteProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

export function CommandPalette({ open, onOpenChange }: CommandPaletteProps) {
  const navigate = useNavigate()

  const run = React.useCallback(
    (fn: () => void) => {
      onOpenChange(false)
      // Defer the action until after the dialog has begun closing so focus
      // restoration doesn't fight the navigation.
      requestAnimationFrame(fn)
    },
    [onOpenChange],
  )

  return (
    <DialogPrimitive.Root open={open} onOpenChange={onOpenChange}>
      <DialogPrimitive.Portal>
        <DialogPrimitive.Overlay
          className={cn(
            'fixed inset-0 z-50 bg-black/70 backdrop-blur-sm',
            'data-[state=open]:animate-in data-[state=closed]:animate-out',
            'data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0',
          )}
        />
        <DialogPrimitive.Content
          aria-label="Command palette"
          className={cn(
            'fixed left-[50%] top-[20%] z-50 w-full max-w-lg translate-x-[-50%]',
            'overflow-hidden rounded-lg border border-border bg-surface-3 shadow-elevation-2',
            'data-[state=open]:animate-in data-[state=closed]:animate-out',
            'data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0',
            'data-[state=closed]:zoom-out-95 data-[state=open]:zoom-in-95',
          )}
        >
          <DialogPrimitive.Title className="sr-only">Command palette</DialogPrimitive.Title>
          <DialogPrimitive.Description className="sr-only">
            Search for a command or jump to a section
          </DialogPrimitive.Description>
          <Command
            loop
            className="[&_[cmdk-group-heading]]:px-3 [&_[cmdk-group-heading]]:py-1.5 [&_[cmdk-group-heading]]:text-xs [&_[cmdk-group-heading]]:font-medium [&_[cmdk-group-heading]]:uppercase [&_[cmdk-group-heading]]:tracking-wide [&_[cmdk-group-heading]]:text-muted-foreground"
          >
            <div className="flex items-center gap-2 border-b border-border px-3">
              <Search className="h-4 w-4 shrink-0 text-muted-foreground" />
              <Command.Input
                autoFocus
                placeholder="Search commands…"
                className="flex h-11 w-full bg-transparent py-3 text-sm outline-none placeholder:text-muted-foreground disabled:cursor-not-allowed disabled:opacity-50"
              />
            </div>
            <Command.List className="max-h-80 overflow-y-auto overflow-x-hidden p-1.5">
              <Command.Empty className="px-3 py-6 text-center text-sm text-muted-foreground">
                No results found.
              </Command.Empty>

              <Command.Group heading="Navigate">
                <PaletteItem
                  icon={<KanbanSquare className="h-4 w-4" />}
                  label="Board"
                  keywords={['tickets', 'kanban', 'home']}
                  onSelect={() => run(() => void navigate({ to: '/' }))}
                />
                <PaletteItem
                  icon={<Brain className="h-4 w-4" />}
                  label="Memory"
                  keywords={['notes', 'recall']}
                  onSelect={() => run(() => void navigate({ to: '/memory' }))}
                />
                <PaletteItem
                  icon={<Boxes className="h-4 w-4" />}
                  label="Scope"
                  keywords={['boundary', 'boundaries']}
                  onSelect={() => run(() => void navigate({ to: '/scope' }))}
                />
                <PaletteItem
                  icon={<Users className="h-4 w-4" />}
                  label="Identity"
                  keywords={['actors', 'people']}
                  onSelect={() => run(() => void navigate({ to: '/identity' }))}
                />
                <PaletteItem
                  icon={<ScrollText className="h-4 w-4" />}
                  label="Audit"
                  keywords={['lineage', 'diff', 'history']}
                  onSelect={() => run(() => void navigate({ to: '/audit' }))}
                />
              </Command.Group>

              <Command.Group heading="Actions">
                <PaletteItem
                  icon={<Plus className="h-4 w-4" />}
                  label="Create ticket"
                  keywords={['new', 'add', 'issue']}
                  onSelect={() =>
                    run(() => void navigate({ to: '/', search: { create: true } as never }))
                  }
                />
                <PaletteItem
                  icon={<Search className="h-4 w-4" />}
                  label="Search memory"
                  keywords={['find', 'query']}
                  onSelect={() =>
                    run(() => {
                      void navigate({ to: '/memory' })
                      // After navigation lands, focus the memory search input
                      // (tagged via the existing `/` shortcut convention).
                      setTimeout(() => {
                        const target = document.querySelector<HTMLInputElement>(
                          '[data-shortcut-target="search"], input[type="search"]',
                        )
                        target?.focus()
                        target?.select?.()
                      }, 120)
                    })
                  }
                />
              </Command.Group>
            </Command.List>
          </Command>
        </DialogPrimitive.Content>
      </DialogPrimitive.Portal>
    </DialogPrimitive.Root>
  )
}

function PaletteItem({
  icon,
  label,
  keywords,
  onSelect,
}: {
  icon: React.ReactNode
  label: string
  keywords?: string[]
  onSelect: () => void
}) {
  return (
    <Command.Item
      value={label}
      keywords={keywords}
      onSelect={onSelect}
      className={cn(
        'flex cursor-pointer select-none items-center gap-2.5 rounded-md px-3 py-2 text-sm outline-none',
        'text-foreground transition-colors',
        'data-[selected=true]:bg-primary/10 data-[selected=true]:text-primary',
        'aria-disabled:pointer-events-none aria-disabled:opacity-50',
      )}
    >
      <span className="text-muted-foreground [[data-selected=true]_&]:text-primary">{icon}</span>
      <span>{label}</span>
    </Command.Item>
  )
}

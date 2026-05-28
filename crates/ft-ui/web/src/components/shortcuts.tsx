/**
 * Global keyboard shortcuts and the help overlay.
 *
 * Shortcuts are wired with `react-hotkeys-hook` so we get:
 *   - automatic input-focus context (the lib skips dispatch when focus is on
 *     an input/textarea/contenteditable by default).
 *   - composable chord keys (`g b`, `g m`, …).
 *   - portable bindings independent of React-Router's outlet.
 *
 * The help overlay is keyed on `?` (Shift+/). It lives in the app shell so
 * the dialog is reachable from any route.
 */
import * as React from 'react'
import { useHotkeys } from 'react-hotkeys-hook'
import { useNavigate } from '@tanstack/react-router'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'

interface ShortcutDef {
  keys: string
  label: string
}

const SHORTCUTS: ShortcutDef[] = [
  { keys: 'c', label: 'Create ticket' },
  { keys: '/', label: 'Focus search / filter' },
  { keys: 'j / k', label: 'Move list selection up/down' },
  { keys: 'g b', label: 'Go to board' },
  { keys: 'g m', label: 'Go to memory' },
  { keys: 'g i', label: 'Go to identity' },
  { keys: 'g a', label: 'Go to audit' },
  { keys: '?', label: 'Show shortcut help' },
]

interface ShortcutsContextValue {
  /** Open the ticket create dialog. Set from app-shell when mounted. */
  openTicketCreate?: () => void
}

const ShortcutsContext = React.createContext<ShortcutsContextValue>({})

export function useShortcutHandlers(): ShortcutsContextValue {
  return React.useContext(ShortcutsContext)
}

/**
 * Provider that owns the global hotkey state and renders the help dialog.
 * Wrap inside `AppShell`. The `setOpenTicketCreate` setter lets the route
 * that owns the ticket create dialog plug into the `c` shortcut.
 */
export function ShortcutsProvider({ children }: { children: React.ReactNode }) {
  const navigate = useNavigate()
  const [helpOpen, setHelpOpen] = React.useState(false)
  const handlersRef = React.useRef<ShortcutsContextValue>({})

  // `c` — opens the ticket create dialog if the current route has registered
  // a handler. Falls back to navigating home (which mounts the board route
  // with the create dialog wiring) so the shortcut still works from any
  // other page.
  useHotkeys(
    'c',
    () => {
      if (handlersRef.current.openTicketCreate) {
        handlersRef.current.openTicketCreate()
      } else {
        void navigate({ to: '/', search: { create: true } as never })
      }
    },
    { preventDefault: true },
  )

  // `/` focuses the first input on the page with `[data-shortcut-target="search"]`
  // (or any visible <input type="search">/<input type="text">) so each route
  // can opt in by tagging its main filter input.
  useHotkeys(
    '/',
    (e) => {
      e.preventDefault()
      const tagged = document.querySelector<HTMLInputElement>(
        '[data-shortcut-target="search"]',
      )
      const fallback = document.querySelector<HTMLInputElement>(
        'input[type="search"], input[type="text"]',
      )
      const target = tagged ?? fallback
      target?.focus()
      target?.select?.()
    },
  )

  // j / k — relies on the active route to handle the list selection. We
  // dispatch a CustomEvent so individual lists can subscribe without coupling
  // to this provider.
  useHotkeys('j', () => window.dispatchEvent(new CustomEvent('ft:list-nav', { detail: 'down' })))
  useHotkeys('k', () => window.dispatchEvent(new CustomEvent('ft:list-nav', { detail: 'up' })))

  useHotkeys('g+b', () => void navigate({ to: '/' }))
  useHotkeys('g+m', () => void navigate({ to: '/memory' }))
  useHotkeys('g+i', () => void navigate({ to: '/identity' }))
  useHotkeys('g+a', () => void navigate({ to: '/audit' }))

  useHotkeys('shift+/', () => setHelpOpen((v) => !v))

  const value = React.useMemo<ShortcutsContextValue>(
    () => ({
      openTicketCreate: handlersRef.current.openTicketCreate,
    }),
    [],
  )

  // Expose a mutator so the board route can register its dialog opener via
  // a side-effect hook (see `useRegisterShortcut`).
  const ctx: ShortcutsContextValue & {
    _register?: (h: Partial<ShortcutsContextValue>) => void
  } = {
    ...value,
    _register: (h) => {
      Object.assign(handlersRef.current, h)
    },
  }

  return (
    <ShortcutsContext.Provider value={ctx}>
      {children}
      <ShortcutsHelpDialog open={helpOpen} onOpenChange={setHelpOpen} />
    </ShortcutsContext.Provider>
  )
}

/**
 * Register a per-route handler (e.g. an `openTicketCreate` callback) so the
 * global `c` hotkey can invoke it while that route is mounted.
 */
export function useRegisterShortcut(handler: Partial<ShortcutsContextValue>) {
  const ctx = React.useContext(ShortcutsContext) as ShortcutsContextValue & {
    _register?: (h: Partial<ShortcutsContextValue>) => void
  }
  React.useEffect(() => {
    ctx._register?.(handler)
    return () => {
      // Clear handler on unmount so subsequent routes don't inherit it.
      ctx._register?.(Object.fromEntries(Object.keys(handler).map((k) => [k, undefined])))
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [handler.openTicketCreate])
}

function ShortcutsHelpDialog({
  open,
  onOpenChange,
}: {
  open: boolean
  onOpenChange: (v: boolean) => void
}) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle className="font-mono">Keyboard shortcuts</DialogTitle>
        </DialogHeader>
        <ul className="space-y-1.5 text-sm">
          {SHORTCUTS.map((s) => (
            <li key={s.keys} className="flex items-center justify-between gap-3">
              <span className="text-muted-foreground">{s.label}</span>
              <kbd className="rounded border border-border/70 bg-muted/60 px-2 py-0.5 font-mono text-xs">
                {s.keys}
              </kbd>
            </li>
          ))}
        </ul>
      </DialogContent>
    </Dialog>
  )
}

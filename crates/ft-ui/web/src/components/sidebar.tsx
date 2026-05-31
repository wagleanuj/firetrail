/**
 * Left navigation sidebar for the app shell (§6 of the redesign spec).
 *
 * Collapsible between a ~220px labelled rail and a 60px icon rail. The
 * collapsed state is persisted to localStorage so it survives reloads. Nav
 * items use TanStack Router `<Link>` with `activeProps` (mirroring the prior
 * top-nav semantics, including `activeOptions={{ exact }}` for the board
 * route). The active item gets a cyan tint plus a left accent bar.
 */
import * as React from 'react'
import { Link } from '@tanstack/react-router'
import {
  KanbanSquare,
  ListTodo,
  Brain,
  Boxes,
  Users,
  FileCog,
  ScrollText,
  Keyboard,
  PanelLeftClose,
  PanelLeftOpen,
  Layers,
  type LucideIcon,
} from 'lucide-react'
import { Button } from '@/components/ui/button'
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@/components/ui/tooltip'
import { cn } from '@/lib/utils'

interface NavItem {
  to: string
  label: string
  icon: LucideIcon
  exact?: boolean
}

const NAV: NavItem[] = [
  { to: '/', label: 'Board', icon: KanbanSquare, exact: true },
  { to: '/backlog', label: 'Backlog', icon: ListTodo },
  { to: '/epics', label: 'Epics', icon: Layers },
  { to: '/memory', label: 'Memory', icon: Brain },
  { to: '/scope', label: 'Scope', icon: Boxes },
  { to: '/identity', label: 'Identity', icon: Users },
  { to: '/profile', label: 'Profile', icon: FileCog },
  { to: '/audit', label: 'Audit', icon: ScrollText },
]

const STORAGE_KEY = 'ft-ui:sidebar-collapsed'

function readCollapsed(): boolean {
  if (typeof window === 'undefined') return false
  try {
    return window.localStorage.getItem(STORAGE_KEY) === '1'
  } catch {
    return false
  }
}

export function Sidebar() {
  const [collapsed, setCollapsed] = React.useState(readCollapsed)

  React.useEffect(() => {
    try {
      window.localStorage.setItem(STORAGE_KEY, collapsed ? '1' : '0')
    } catch {
      /* ignore persistence failures (private mode, etc.) */
    }
  }, [collapsed])

  function openShortcutsHelp() {
    // Reuse the existing `?` (shift+/) shortcut so we don't fork the overlay.
    window.dispatchEvent(
      new KeyboardEvent('keydown', { key: '?', shiftKey: true, bubbles: true }),
    )
  }

  return (
    <TooltipProvider delayDuration={300}>
      <nav
        aria-label="Primary"
        data-collapsed={collapsed}
        className={cn(
          'flex h-full shrink-0 flex-col border-r border-border/60 bg-card/40 backdrop-blur',
          'transition-[width] duration-150 ease-out',
          collapsed ? 'w-[60px]' : 'w-[220px]',
        )}
      >
        {/* Wordmark + glowing cyan dot */}
        <div
          className={cn(
            'flex h-14 items-center gap-2 px-4',
            collapsed && 'justify-center px-0',
          )}
        >
          <Link to="/" className="flex items-center gap-2 overflow-hidden" aria-label="firetrail home">
            <span className="inline-block h-2.5 w-2.5 shrink-0 rounded-full bg-primary shadow-glow" />
            {!collapsed && (
              <span className="truncate font-display text-sm font-semibold tracking-tight">
                firetrail
              </span>
            )}
          </Link>
        </div>

        <ul className="flex flex-1 flex-col gap-1 px-2 py-2">
          {NAV.map((item) => {
            const Icon = item.icon
            const link = (
              <Link
                to={item.to}
                activeProps={{
                  className: 'bg-primary/10 text-primary before:opacity-100',
                  'aria-current': 'page',
                }}
                activeOptions={{ exact: item.exact }}
                className={cn(
                  'group relative flex items-center gap-3 rounded-md px-3 py-2 text-sm text-muted-foreground transition-colors',
                  'hover:bg-surface-2 hover:text-foreground',
                  // left accent bar; hidden until active via activeProps
                  'before:absolute before:left-0 before:top-1/2 before:h-5 before:w-0.5 before:-translate-y-1/2 before:rounded-full before:bg-primary before:opacity-0 before:transition-opacity before:content-[""]',
                  collapsed && 'justify-center px-0',
                )}
              >
                <Icon className="h-4 w-4 shrink-0" />
                {!collapsed && <span className="truncate">{item.label}</span>}
              </Link>
            )
            return (
              <li key={item.to}>
                {collapsed ? (
                  <Tooltip>
                    <TooltipTrigger asChild>{link}</TooltipTrigger>
                    <TooltipContent side="right">{item.label}</TooltipContent>
                  </Tooltip>
                ) : (
                  link
                )}
              </li>
            )
          })}
        </ul>

        <div
          className={cn(
            'flex items-center gap-1 border-t border-border/60 px-2 py-2',
            collapsed ? 'flex-col' : 'justify-between',
          )}
        >
          <Button
            size="icon"
            variant="ghost"
            className="h-8 w-8"
            aria-label="Show keyboard shortcuts"
            title="Keyboard shortcuts (?)"
            onClick={openShortcutsHelp}
          >
            <Keyboard className="h-4 w-4" />
          </Button>
          <Button
            size="icon"
            variant="ghost"
            className="h-8 w-8"
            aria-label={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}
            aria-expanded={!collapsed}
            title={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}
            onClick={() => setCollapsed((v) => !v)}
          >
            {collapsed ? (
              <PanelLeftOpen className="h-4 w-4" />
            ) : (
              <PanelLeftClose className="h-4 w-4" />
            )}
          </Button>
        </div>
      </nav>
    </TooltipProvider>
  )
}

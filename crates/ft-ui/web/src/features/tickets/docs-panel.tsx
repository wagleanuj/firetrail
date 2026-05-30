/**
 * Ticket-drawer **Docs** panel (firetrail-2mwp.8).
 *
 * Lists the `DocumentedIn` docs for a ticket, each rendered inline (markdown)
 * with a freshness badge driven by the server's `content_hash` drift check.
 * Editing a doc writes through ops — the file is rewritten, the record
 * re-indexed synchronously, and a `stale` badge flips back to fresh on save.
 */
import { useState } from 'react'
import { AlertTriangle, FileText, Loader2, Pencil } from 'lucide-react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import { Skeleton } from '@/components/ui/skeleton'
import { cn } from '@/lib/utils'
import type { DocView } from '@/api/types/DocView'
import type { DocFreshnessView } from '@/api/types/DocFreshnessView'
import { DescriptionEditor } from './description-editor'
import { useSaveDoc, useTicketDocsQuery } from './use-ticket-docs'

export function DocsPanel({ ticketId }: { ticketId: string }) {
  const { data, isLoading, error } = useTicketDocsQuery(ticketId)

  return (
    <div className="space-y-2">
      <Label className="flex items-center gap-1.5">
        <FileText className="h-3.5 w-3.5" />
        Docs
      </Label>

      {isLoading ? (
        <Skeleton className="h-20 w-full" />
      ) : error ? (
        <p className="rounded-md border border-destructive/40 bg-destructive/5 px-3 py-3 text-sm text-destructive">
          Failed to load docs: {(error as Error).message}
        </p>
      ) : !data || data.length === 0 ? (
        <p className="rounded-md border border-dashed border-border/60 px-3 py-3 text-sm text-muted-foreground">
          No documentation linked.
        </p>
      ) : (
        <ul className="space-y-3">
          {data.map((doc) => (
            <DocCard key={doc.id} ticketId={ticketId} doc={doc} />
          ))}
        </ul>
      )}
    </div>
  )
}

/** Freshness pill — rendered only when the doc is not fresh. */
function FreshnessBadge({ id, freshness }: { id: string; freshness: DocFreshnessView }) {
  if (freshness === 'fresh') return null
  const stale = freshness === 'stale'
  return (
    <Badge
      data-testid={`doc-badge-${id}`}
      variant={stale ? 'outline' : 'destructive'}
      className={cn(
        'gap-1 capitalize',
        stale && 'border-amber-400/40 bg-amber-400/10 text-amber-300',
      )}
      title={
        stale
          ? 'The file changed since it was last indexed — save to re-index.'
          : 'The linked file is missing — a broken link.'
      }
    >
      <AlertTriangle className="h-3 w-3" />
      {freshness}
    </Badge>
  )
}

function DocCard({ ticketId, doc }: { ticketId: string; doc: DocView }) {
  const [editing, setEditing] = useState(false)
  const [draft, setDraft] = useState(doc.content)
  const save = useSaveDoc(ticketId)

  return (
    <li className="space-y-2 rounded-md border border-border/50 bg-background/60 p-3">
      <div className="flex flex-wrap items-center gap-2">
        <span className="font-display text-sm font-semibold leading-snug">{doc.title}</span>
        <Badge variant="secondary" className="capitalize">
          {doc.doc_type}
        </Badge>
        <FreshnessBadge id={doc.id} freshness={doc.freshness} />
        {!editing && (
          <Button
            type="button"
            size="sm"
            variant="ghost"
            className="ml-auto h-7 gap-1.5 text-xs"
            data-testid={`doc-edit-${doc.id}`}
            onClick={() => {
              setDraft(doc.content)
              setEditing(true)
            }}
          >
            <Pencil className="h-3 w-3" />
            Edit
          </Button>
        )}
      </div>

      <p className="font-mono text-[0.65rem] text-muted-foreground">{doc.path}</p>

      {editing ? (
        <div className="space-y-2">
          <DescriptionEditor value={draft} onChange={setDraft} />
          <div className="flex justify-end gap-2">
            <Button
              type="button"
              size="sm"
              variant="ghost"
              onClick={() => setEditing(false)}
              disabled={save.isPending}
            >
              Cancel
            </Button>
            <Button
              type="button"
              size="sm"
              data-testid={`doc-save-${doc.id}`}
              disabled={save.isPending}
              onClick={async () => {
                await save.mutateAsync({ docId: doc.id, content: draft })
                setEditing(false)
              }}
            >
              {save.isPending && <Loader2 className="mr-1 h-3 w-3 animate-spin" />}
              Save
            </Button>
          </div>
        </div>
      ) : doc.freshness === 'missing' ? (
        <p className="rounded-md border border-dashed border-destructive/40 px-3 py-2 text-xs text-muted-foreground">
          Linked file not found on disk.
        </p>
      ) : (
        <DescriptionEditor value={doc.content} editable={false} />
      )}
    </li>
  )
}

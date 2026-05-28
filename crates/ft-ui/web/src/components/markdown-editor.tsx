/**
 * Tiptap-backed Markdown editor — shared between the tickets surface
 * (ticket detail drawer + create modal) and the memory surface
 * (read-only render + memory create modal).
 *
 * Storage: **Markdown** (via `tiptap-markdown`). The backend persists the
 * description/body as a plain string, which the CLI's `ft show` and
 * `ft memory show` will print verbatim — keeping the source-of-truth
 * Markdown means terminal output stays human-readable instead of being
 * raw HTML/JSON.
 *
 * Modes:
 *   - `editable={true}`: full toolbar, focus ring, user edits the doc.
 *   - `editable={false}`: rendered-only — no toolbar, no caret.
 *
 * Originally lived in `features/tickets/description-editor.tsx` but was
 * promoted in Wave 2-C so the memory routes (where the long-form body
 * is the marquee piece) can reuse the same prose styling and toolbar.
 */
import { useEditor, EditorContent, type Editor } from '@tiptap/react'
import StarterKit from '@tiptap/starter-kit'
import Placeholder from '@tiptap/extension-placeholder'
import Link from '@tiptap/extension-link'
import CodeBlockLowlight from '@tiptap/extension-code-block-lowlight'
import { Markdown } from 'tiptap-markdown'
import { common, createLowlight } from 'lowlight'
import { useEffect } from 'react'
import { cn } from '@/lib/utils'
import { Button } from '@/components/ui/button'

const lowlight = createLowlight(common)

export interface MarkdownEditorProps {
  value: string
  onChange?: (markdown: string) => void
  editable?: boolean
  placeholder?: string
  className?: string
}

/**
 * Hook returning a configured editor. Exposed so callers can read
 * `.storage.markdown.getMarkdown()` directly when serializing on submit.
 */
export function useMarkdownEditor({
  value,
  onChange,
  editable = true,
  placeholder = 'Describe…',
}: MarkdownEditorProps): Editor | null {
  const editor = useEditor({
    extensions: [
      StarterKit.configure({ codeBlock: false }),
      CodeBlockLowlight.configure({ lowlight }),
      Link.configure({ openOnClick: false, autolink: true }),
      Placeholder.configure({ placeholder }),
      Markdown.configure({ html: false, linkify: true, breaks: true }),
    ],
    content: value,
    editable,
    editorProps: {
      attributes: {
        class: cn(
          'prose prose-invert prose-sm max-w-none focus:outline-none',
          'min-h-[6rem]',
        ),
      },
    },
    onUpdate: ({ editor }) => {
      if (!onChange) return
      const md =
        (editor.storage as { markdown?: { getMarkdown(): string } }).markdown?.getMarkdown() ?? ''
      onChange(md)
    },
  })

  // Sync external `value` changes (e.g. switching tickets) — Tiptap doesn't
  // re-mount on prop change by default.
  useEffect(() => {
    if (!editor) return
    const current =
      (editor.storage as { markdown?: { getMarkdown(): string } }).markdown?.getMarkdown() ?? ''
    if (current !== value) {
      editor.commands.setContent(value, { emitUpdate: false })
    }
  }, [editor, value])

  useEffect(() => {
    if (!editor) return
    if (editor.isEditable !== editable) editor.setEditable(editable)
  }, [editor, editable])

  return editor
}

export function MarkdownEditor(props: MarkdownEditorProps) {
  const editor = useMarkdownEditor(props)
  const { editable = true, className } = props
  return (
    <div
      className={cn(
        'rounded-md border border-border bg-background/60 px-3 py-2',
        editable && 'focus-within:ring-2 focus-within:ring-ring',
        className,
      )}
    >
      {editor && editable && <Toolbar editor={editor} />}
      <EditorContent editor={editor} />
    </div>
  )
}

function Toolbar({ editor }: { editor: Editor }) {
  const btn = (active: boolean) =>
    cn(
      'h-7 px-2 text-xs font-mono',
      active ? 'bg-primary/20 text-primary' : 'text-muted-foreground',
    )
  return (
    <div className="mb-2 flex flex-wrap items-center gap-1 border-b border-border/60 pb-2">
      <Button
        type="button"
        size="sm"
        variant="ghost"
        className={btn(editor.isActive('bold'))}
        onClick={() => editor.chain().focus().toggleBold().run()}
      >
        B
      </Button>
      <Button
        type="button"
        size="sm"
        variant="ghost"
        className={cn(btn(editor.isActive('italic')), 'italic')}
        onClick={() => editor.chain().focus().toggleItalic().run()}
      >
        I
      </Button>
      <Button
        type="button"
        size="sm"
        variant="ghost"
        className={btn(editor.isActive('code'))}
        onClick={() => editor.chain().focus().toggleCode().run()}
      >
        {'<>'}
      </Button>
      <Button
        type="button"
        size="sm"
        variant="ghost"
        className={btn(editor.isActive('codeBlock'))}
        onClick={() => editor.chain().focus().toggleCodeBlock().run()}
      >
        ```
      </Button>
      <Button
        type="button"
        size="sm"
        variant="ghost"
        className={btn(editor.isActive('bulletList'))}
        onClick={() => editor.chain().focus().toggleBulletList().run()}
      >
        •
      </Button>
      <Button
        type="button"
        size="sm"
        variant="ghost"
        className={btn(editor.isActive('orderedList'))}
        onClick={() => editor.chain().focus().toggleOrderedList().run()}
      >
        1.
      </Button>
      <Button
        type="button"
        size="sm"
        variant="ghost"
        className={btn(editor.isActive('link'))}
        onClick={() => {
          const previous = editor.getAttributes('link').href as string | undefined
          const url = window.prompt('URL', previous ?? 'https://')
          if (url === null) return
          if (url === '') {
            editor.chain().focus().unsetLink().run()
            return
          }
          editor.chain().focus().extendMarkRange('link').setLink({ href: url }).run()
        }}
      >
        link
      </Button>
    </div>
  )
}

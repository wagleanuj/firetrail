/**
 * Backwards-compatible re-export of the shared Markdown editor.
 *
 * The original Tiptap component lived here in Wave 1; Wave 2-C promoted it
 * to `@/components/markdown-editor` so the memory routes could reuse the
 * same prose styling. We keep this file as a thin alias so the ticket
 * surface's existing imports keep working and the public API stays stable.
 */
export {
  MarkdownEditor as DescriptionEditor,
  useMarkdownEditor as useDescriptionEditor,
  type MarkdownEditorProps,
} from '@/components/markdown-editor'

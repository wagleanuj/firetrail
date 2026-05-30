#!/usr/bin/env sh
# scripts/hooks/doc-index-warmer.sh — post-commit doc index warmer (firetrail-2mwp.9).
#
# Keeps the search index warm after a commit touches markdown docs, so a fresh
# `firetrail search` / `prime` reflects the new content without anyone running
# `firetrail doc index` by hand.
#
# Lazy-on-read freshness (the `content_hash` check in prime/search) already
# guarantees *correctness*; this hook is a latency optimisation — it pays the
# re-index cost once, at commit time, instead of on the next read.
#
# Design:
#   - Best-effort: any failure is swallowed (`|| true`) so it can NEVER block or
#     fail a commit.
#   - Cheap gate: only runs when the commit actually changed a `.md` file.
#   - `firetrail doc index` (no target) re-reads every doc record but only
#     rewrites the ones whose file hash drifted, so the warm pass is idempotent
#     and skips unchanged docs.
#
# Opt out for a single commit with FIRETRAIL_NO_DOC_WARMER=1.

[ -n "$FIRETRAIL_NO_DOC_WARMER" ] && exit 0

# No binary on PATH (e.g. a clone that hasn't built/installed firetrail) → skip.
command -v firetrail >/dev/null 2>&1 || exit 0

# Did this commit touch any markdown? (HEAD is the just-created commit.)
changed_md=$(git diff-tree --no-commit-id --name-only -r HEAD 2>/dev/null \
    | grep -iE '\.md$' || true)
[ -z "$changed_md" ] && exit 0

echo "firetrail: warming doc index (post-commit)"
firetrail doc index >/dev/null 2>&1 || true

exit 0

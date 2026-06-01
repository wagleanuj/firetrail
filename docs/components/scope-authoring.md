---
doc_type: reference
status: draft
links:
  - firetrail-jr02
---

# scope-authoring — the write path for `.firetrail/scopes.yaml`

**Crates touched:** ft-scope, ft-ops, ft-cli, ft-ui
**ADR:** `docs/decisions/0004-multi-scope-records.md` (authoring addendum)
**Related spec:** `docs/specs/2026-05-31-per-scope-profiles-design.md`

---

## Purpose

Scopes are the per-package axis the monorepo story keys on: a record's
`owningScope` resolves review authority, CODEOWNERS routing follows the scope
that governs a path, and per-scope profiles (the spec above) bind a package's
validate/test/build/lint commands to a scope id. All three resolve the same way
— **last-declared-wins** over `.firetrail/scopes.yaml`, identical to CODEOWNERS.

`ft-scope`'s `registry` module already *reads* that file into a compiled
`ScopeRegistry`. This component is the *write* path: an order-stable,
regenerate-the-block writer plus the CLI, API, and UI surfaces that drive it.

**Progressive disclosure.** A standalone repo never needs scopes. The file is
never auto-created — a missing file yields an empty registry and every path
resolves with no owning scope. The words "scope", "precedence", and "shadow"
never surface until a team opts in. See ADR-0004's authoring addendum.

---

## The writer (`ft-scope::writer`)

Operates on the raw `ScopesFile` / `ScopeYaml` model (not the compiled
registry). Two principles:

- **Order is semantic.** Resolution is last-declared-wins, so every operation
  preserves declaration order. New scopes are *appended* (they become
  last-declared, i.e. highest precedence); upserts replace **in place**.
- **Regenerate the block.** On save the whole file is re-serialized
  deterministically and a tool-managed header comment is prepended. Hand-written
  comments are **not** preserved (accepted for v1 — the file is tool-managed and
  the header says so).

| Function | Purpose |
|---|---|
| `load_file(root) -> ScopesFile` | Read `<root>/.firetrail/scopes.yaml`. A missing file is **not** an error — it yields an empty `ScopesFile`. |
| `save_file(root, file)` | Run `validate` first, then write the header + deterministically serialized model, creating `.firetrail/` if needed. An invalid model never reaches disk. |
| `upsert_scope(file, scope)` | Replace the scope with the same id in place (position preserved), else append it (becomes last-declared). |
| `remove_scope(file, id)` | Drop the scope by id; `ScopeNotFound` if absent. |
| `reorder(file, ordered_ids)` | Reorder to `ordered_ids`, which must be a permutation of the existing ids; `ReorderMismatch` otherwise. |
| `validate(file)` | Pre-write checks: every `applies_to` glob compiles, ids are unique, aliases are unique across all scopes, and every scope declares at least one `applies_to` pattern. |

The header written on every save:

```
# Managed by `firetrail scope`. Order matters: resolution is last-declared-wins.
```

---

## CLI surface (`firetrail scope`)

The pre-existing read-only verbs (`list`, `show`, `aliases`, `owners`) are
unchanged. The authoring verbs:

| Command | Purpose |
|---|---|
| `firetrail scope add <id> --applies-to <glob>… [--name <s>] [--alias <a>]… [--codeowners <path>]` | Append a new scope (`--applies-to` repeatable, at least one required). The new scope is last-declared, so declare broad scopes first. |
| `firetrail scope edit <id> [--applies-to <glob>]… [--name <s>] [--alias <a>]… [--codeowners <path>] [--clear-name] [--clear-codeowners]` | Edit in place; only fields passed change. `--applies-to` / `--alias` **replace** the stored list when given (omit to keep it). `--clear-name` / `--clear-codeowners` unset the optional field. |
| `firetrail scope rm <id>` | Remove a scope by id. |
| `firetrail scope reorder <id>…` | Reorder to the given full id list (must be a permutation). Order is precedence — this is the lever for changing it. |

---

## API surface (`/api/scope`, ft-ui → ft-ops)

The read routes (`GET /api/scope`, `/aliases`, `/owners`, `/:id`) carry over
from Wave 3. The authoring routes call the matching `ft_ops::scope` op, which
wraps the writer:

| Route | Op | Notes |
|---|---|---|
| `POST /api/scope` | `scope::add` | Append a complete new scope. 409 on a duplicate id. |
| `PUT /api/scope/:id` | `scope::edit` | Partial edit — only provided fields change. 404 when absent. |
| `DELETE /api/scope/:id` | `scope::remove` | Remove by id. 404 when absent. |
| `POST /api/scope/reorder` | `scope::reorder` | Reorder to the full ordered id list. |
| `GET /api/scope/preview` | `scope::preview` | Per-scope tracked-file **match counts** plus coverage **warnings** for the live editor. |

`preview` walks the tracked files at `HEAD` and, per scope, counts matches in
declaration order. It emits two advisory warnings (both require a populated
tree):

- **Zero-match** — a scope whose globs match no tracked file.
- **Shadow** — a broad scope declared *after* a narrower one whose matched
  files are a strict subset. Last-declared-wins means the broad scope always
  wins where the narrow one would have, so the narrow scope never governs a
  file. This is the classic CODEOWNERS footgun.

---

## UI surface (scope-explorer)

The scope-explorer route gives create / edit / delete / reorder over the API
above, with:

- **Glob + directory autocomplete** — a file-path combobox backed by the file
  index, plus a "glob from a directory" affordance that turns a picked
  directory into `<dir>/**`.
- **Live preview** — match counts and the zero-match / shadow warnings from
  `GET /api/scope/preview`, updated as the user edits.
- **Standalone empty state (progressive disclosure)** — a repo with no scopes
  renders a calm explanation that scopes are only for monorepos that need
  separate ownership or validation, with an opt-in "Add a scope". The file is
  never created until the user adds one.
- **Suggest-only monorepo scaffold** — proposes scope candidates from the
  repo's package directories (well-known monorepo roots first, else first-level
  directories), each mapping its id to the directory and matching `<dir>/**`.
  Suggestions only: every candidate is confirmed by hand before it is written.

---

## References

- ADR-0004 — Multi-scope records (the authoring addendum: last-declared-wins,
  progressive disclosure, YAML round-trip)
- `docs/specs/2026-05-31-per-scope-profiles-design.md` — per-scope profiles
  built on this scope axis (`firetrail-jr02`)
- `docs/ARCHITECTURE.md` — "Multi-scope routing" section
- `docs/components/ft-cli.md`

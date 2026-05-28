# Firetrail GUI — Design

**Date:** 2026-05-28
**Status:** Brainstorm complete; awaiting implementation kickoff.
**Scope target:** V1-Complete — all 23 UI-relevant commands wired through a local web app, with full read+write parity.

## Summary

Ship a local web UI for firetrail as a new in-repo crate `ft-ui`. It serves an axum HTTP/JSON API and an embedded React+shadcn SPA in a single binary, invoked via `ft ui`. To avoid duplicating logic between the CLI and the UI, every relevant CLI command is incrementally extracted into a new transport-agnostic `ft-ops` crate. The CLI and the HTTP server both become thin adapters over `ft-ops`.

## Decisions

| # | Decision | Choice |
|---|----------|--------|
| 1 | Repo home | In-repo, new workspace member |
| 2 | API surface | New axum server inside `ft-ui` (not via existing embed-daemon, which stays scoped to embeddings) |
| 3 | UI scope | C2 — full read+write for the ~23 commands that make sense in a browser; setup/destructive long tail (`init`, `hook`, `migrate`, `merge_driver`, `daemon`, `import`, `promote_import`, `compact`, `sync_cmd`, `server_hooks`) stays CLI-only |
| 4 | Code structure | Incremental extraction into `ft-ops`; per-command, alongside the UI work that needs it |
| 5 | Delivery | W2 — vertical workflow slices (tickets → memory → audit/scope/identity) |
| 6 | Auth on loopback | Token-bootstrapped SameSite=Strict signed session cookie + `Origin`/`Host` checks |
| 7 | Real-time updates | SSE from an `EventBus` in `ft-ops`, with 10s polling fallback for cross-process freshness |
| 8 | Workspace scoping | Single workspace per `ft ui` invocation |
| 9 | Frontend stack | Vite + React + TypeScript + shadcn/ui + TanStack Query + TanStack Router + dnd-kit + Zod |
| 10 | Type sharing | `ts-rs` codegen from `ft-ops` types into `web/src/api/types/`, CI-enforced |

## Architecture

### Crate layout

```
crates/
  ft-ops/                  # NEW. Transport-agnostic command bodies.
    src/lib.rs
    src/tickets/           # board, list, show, create, claim, update, close, link, transition
    src/memory/            # views, create, salvage, search, similar
    src/scope/             # edit, list
    src/identity/          # show, update, trust
    src/audit/             # lint, verify, review, criteria, diff, graph
    src/events.rs          # tokio broadcast EventBus emitted from every write op
  ft-ui/                   # NEW. Axum server + embedded SPA.
    build.rs               # runs pnpm build in web/ if dist/ is stale
    src/main.rs            # bin: invoked via `ft ui`
    src/server.rs          # axum app, auth middleware, SSE handler, heartbeat exit
    src/routes/            # one module per ft-ops domain
    src/assets.rs          # rust-embed of web/dist/
    web/                   # Vite + React + shadcn (own pnpm/package.json)
  ft-cli/
    src/commands/ui.rs     # NEW. Spawns ft-ui binary, opens browser.
    src/commands/*.rs      # SHRINKS over time. Becomes clap → ft-ops adapter.
```

`ft-cli` and `ft-ui` ship as two binaries. `ft ui` shells out to the `ft-ui` binary so the CLI's compile tree stays free of axum/tower/rust-embed.

### Boundary contract for `ft-ops`

Every op is a pure function `fn op(ws: &Workspace, identity: &Identity, input: I, events: &EventBus) -> Result<O, OpsError>`.

Forbidden inside `ft-ops`:
- `println!` / `eprintln!` / reading stdin
- `clap` or `axum` types
- `std::env::current_dir()` or other ambient context
- HTTP-specific or CLI-specific error types

`OpsError` is a typed enum (`NotFound`, `Conflict`, `PermissionDenied`, `Validation { field, reason }`, `Internal(anyhow::Error)`). Transports map it to their own error shapes.

Interactive prompts in `crates/ft-cli/src/prompt.rs` (the `qpi`/`tl4`/`iqs` wiring) become CLI-only. HTTP requests carry the equivalent intent as explicit fields (`takeover: bool`, `accept: ["mem-1", "mem-3"]`, etc.).

### Server

- Binds `127.0.0.1:0`, prints `http://127.0.0.1:PORT/?token=...`, opens the browser.
- Lifetime tied to the process; SPA pings `/api/heartbeat` every 20s, server exits after 60s of silence.
- Auth middleware on `/api/*`: valid signed cookie, `Origin` same-origin or absent, `Host` matches loopback URL. Bootstrap token is single-use, expires 60s after startup.
- SSE on `GET /api/events`. Subscribes to `ft-ops::events::EventBus` (`tokio::sync::broadcast`). Replay supported via `Last-Event-Id` and a small ring buffer.

### Frontend

- shadcn components copied into `src/components/ui/` (we own them).
- TanStack Query for server state; optimistic updates on writes; SSE events drive cache invalidation, coalesced with a client-generated `request_id` echoed by the server to avoid mid-animation re-renders during drag-and-drop.
- TanStack Router (file-based).
- dnd-kit for the kanban.
- ts-rs types are imported directly; no hand-written API types.

### Dev workflow

```
just ui-dev    # pnpm dev (Vite :5173) + cargo run -p ft-ui --dev (axum :5174)
               # Vite proxies /api/* and /api/events to :5174
just ui-build  # pnpm build, then cargo build -p ft-ui (release)
just ui        # ui-build, then runs `ft ui`
```

`build.rs` runs `pnpm install && pnpm build` only if `web/dist/` is missing or stale. Gated behind a `bundled-ui` cargo feature so `cargo check` and unrelated work skip it entirely.

## Wave plan (W2)

### Wave 0 — Foundations (~1 week)

- `ft-ops` skeleton: `Workspace`, `Identity`, `OpsError`, `EventBus`.
- `ft-ui` skeleton: axum app, auth middleware, SSE handler, rust-embed wiring, heartbeat exit.
- `web/` Vite + React + shadcn scaffold, TanStack Query/Router, ts-rs codegen pipeline, justfile targets.
- `ft ui` subcommand in `ft-cli` that spawns the binary.
- Smoke test: server boots, browser opens, `/api/workspace` works, SSE channel stays open.

### Wave 1 — Ticket lifecycle (~4–5 weeks; the kanban)

- Ops extraction: `board`, `list`, `show`, `create`, `claim`, `update`, `close`, `link`, `transition`.
- HTTP routes for the above.
- React: board page (dnd-kit kanban), ticket detail drawer, create modal, optimistic updates, SSE invalidation.
- Integration tests at ops level and HTTP level; Playwright happy-path: create → claim → drag-to-done.
- **Exit criteria:** team uses the UI for daily ticket triage for one full week before Wave 2 starts.

### Wave 2 — Memory (~3–4 weeks)

- Ops extraction: `memory_views`, `memory_create`, `salvage`, `search`, `similar`. Search ops auto-spawns the existing embed-daemon via the same `ensure_running` pattern the CLI uses today.
- HTTP routes + SSE events for memory writes.
- React: memory browser (list/filter/tags), detail, create form, salvage workflow (interactive prompts become explicit accept/reject UI), search page with semantic + keyword toggle.

### Wave 3 — Scope, identity, audit (~4 weeks)

- Ops extraction: `scope`, `identity`, `trust`, `lint`, `verify`, `review`, `criteria`, `diff`, `graph`.
- HTTP routes + SSE.
- React: scope editor (CODEOWNERS-aware), identity panel + capability matrix, audit dashboard (lint findings, verify report, review queue), diff viewer, small force-directed graph view.
- **Exit criteria for V1-Complete:** all 23 commands reachable from the UI; no UI feature flagged "preview"; tests green; `ft ui` documented in README.

**Total estimate:** 12–14 weeks for one contributor; 7–9 weeks for two parallel.

## Testing

- **`ft-ops` unit tests** — every op against `ft-testkit` fixture workspaces. Invariants live here.
- **`ft-ui` HTTP integration tests** — boot axum on ephemeral port, hit with `reqwest`, assert JSON + status + SSE frames. Auth middleware gets its own suite.
- **Playwright E2E** — one happy-path per wave, runs against `just ui-build` output in CI.
- **ts-rs drift guard** — CI runs `cargo xtask gen-ts && git diff --exit-code`.

## Observability

- `tracing` to stderr; JSON in CI, pretty in dev. Per-request spans with method/path/status/duration.
- SSE event bus tees into `tracing`, so the mutation stream is greppable in dev.
- No external telemetry in v1.

## Risks and mitigations

1. **Ops extraction surfaces hidden coupling** (stdout side effects, exit-code reliance, implicit cwd). → Extract each command in a single PR with its tests; never batch.
2. **SQLite lock contention CLI ↔ UI.** → Confirm WAL mode; keep ops transactions short; document the constraint until measured. Move to a writer-singleton later if needed.
3. **`build.rs` slows `cargo build`.** → Run only when `web/dist/` is stale; skip on `cargo check`; gate behind `bundled-ui` cargo feature.
4. **shadcn version drift.** → Pin the shadcn CLI version; document the manual update procedure.
5. **dnd-kit + SSE re-render race.** → Optimistic updates hold the new state; the server echoes a client-generated `request_id` so the originator can ignore its own SSE event.

## What is explicitly out of scope for V1

- Multi-workspace switcher (URLs are flat; add `/api/ws/:slug/` later if it earns its keep).
- Remote daemon / non-loopback access.
- Setup/destructive commands in the UI: `init`, `hook`, `server_hooks`, `migrate`, `doctor`, `merge_driver`, `daemon`, `compact`, `import`, `promote_import`, `sync_cmd`. They remain CLI-only.
- Telemetry, analytics, accounts.
- Mobile layouts (desktop browsers only).

---
doc_type: design
status: draft
scope: ft-cli
links:
  - firetrail-lj41
---

# Repo Profile + Bootstrap + Validate-Command — Design

> **Status:** Draft — awaiting review
> **Sub-project A** of the larger "living architecture, rules & governance" initiative.

## Context

We want firetrail (installed into a *host* repo `X`) to understand `X` well enough
to support discussion-driven architecture docs, repo rules, drift detection, and a
post-execution audit loop. All of these need a small, always-available set of repo
facts to read from: how to validate a change, what the standard commands are, what
languages/tooling are in play, and a shallow map of the repo's components.

This spec covers **only that foundation** — the *repo profile* and how it gets
bootstrapped. Rich per-component architecture docs (sub-project B), repo rules
(C), drift detection (D), the audit loop (E), and the rules/docs UI (F) are
separate specs that build on this one.

### Decisions already settled

- **Storage model:** profile and other firetrail artifacts are **first-class
  records**, not loose markdown.
- **Where artifacts live:** in the **external memory repo** (firetrail's external
  storage mode), keeping host repo `X` clean as feature docs accumulate.
- **Division of labor (the backbone principle):** this is a direct application of
  **ADR-0005** — *firetrail produces structured context and enforces structural
  guardrails; the host agent does the reasoning.* The AI agent inspects `X`,
  discusses with the user, and **decides** the profile contents. Firetrail
  **stores, indexes, validates, and surfaces** those decisions. Firetrail ships
  **no** language/tooling auto-detection heuristics in Rust — that judgment lives
  in a skill the agent follows.

| Piece | Agent (judgment) | Firetrail (mechanism) |
|---|---|---|
| Profile (A) | Inspect repo, propose + confirm commands/components | Store as `RepoProfile` record; surface to tools; `doctor` warns if missing |

## Goals

- Persist a single, lightweight `RepoProfile` per host repo, holding the validate
  command, the standard test/build/lint commands, language/tooling facts,
  and a shallow component map (names + paths only).
- Provide a `firetrail profile` CLI surface the agent calls to persist its
  decisions, with partial-update semantics.
- Bootstrap the profile via an agent-run conversation (extended
  `firetrail-bootstrap` skill), never via firetrail-side detection.
- Use the existing trust lifecycle (Draft → Reviewed → Verified) as the
  "agent proposed → human confirmed" signal.
- Nudge persistently toward a validate command without hard-blocking the workflow.
- Expose read/edit of the profile in the web UI.

## Non-goals

- Rich per-component architecture docs (sub-project B — note: firetrail already
  has the `Doc` record + `content_hash` drift primitive + `firetrail doc index`,
  so B is smaller than it looks).
- Repo rules and the brainstorm-to-rules flow (C).
- The audit loop itself (E) — this spec only provides the validate command it
  will consume, and defines graceful degradation when it's absent.
- Per-scope profiles for monorepos with different validate commands per package
  (future extension; YAGNI for v1).

## Design

### 1. The `RepoProfile` record

A new `RecordKind::RepoProfile`, added the standard centralized way (the
synchronized edits across `ft-core` `id.rs`/`record.rs` and `ft-storage`'s
`ALL_KINDS` arrays for both backends). It reuses the existing `RecordEnvelope`
wholesale (`id`, `title`, `owner`, `created_by`, `origin`, `state_hash` chain,
`prev_state_hash`, `history`, `applies_to`, `labels`, `trust`). Only the body is
new:

```rust
struct RepoProfileBody {
    // Commands — agent-decided; the audit loop (E) consumes `validate_command`.
    validate_command: Option<String>,   // the canonical "prove it's good" command
    test_command:     Option<String>,
    build_command:    Option<String>,
    lint_command:     Option<String>,
    // (No standalone format command: formatting belongs inside `validate`/`lint`,
    //  e.g. `cargo fmt --check && cargo test && cargo clippy`.)

    // Tooling facts
    languages:        Vec<String>,       // e.g. ["rust", "typescript"]
    package_managers: Vec<String>,       // e.g. ["cargo", "pnpm"]
    runtime:          Option<String>,    // e.g. "node 20"

    // Shallow component map (names + paths only; rich docs are sub-project B)
    components:       Vec<ComponentRef>, // { name, path, summary? }

    notes:            Option<String>,    // free-text the agent/user persists
    trust:            TrustState,        // Draft → Reviewed → Verified
}

struct ComponentRef {
    name:    String,
    path:    String,
    summary: Option<String>,
}
```

**Trust lifecycle does the propose→confirm work for free.** The agent writes the
profile as `Draft` (`origin = Agent`) — its proposal. The user confirming (in
discussion or UI) transitions it to `Reviewed`/`Verified` via the existing
`ft-trust` machinery. No new state machine.

**Singleton, by convention.** One `RepoProfile` per repo. `firetrail profile set`
updates the existing record in place if present (new `state_hash`, chained via
`prev_state_hash`), else creates it. `firetrail doctor` warns if it finds zero or
more than one.

### 2. The `firetrail profile` CLI surface

The API the bootstrap skill calls. Partial-update throughout, so the agent fills
the profile in incrementally as the discussion unfolds.

```
firetrail profile show [--json]
    Print the current profile. --json for agent/machine consumption.
    Exit non-zero if no profile exists.

firetrail profile set [--validate <cmd>] [--test <cmd>] [--build <cmd>]
                      [--lint <cmd>]
                      [--language <lang>]...        (repeatable)
                      [--package-manager <pm>]...   (repeatable)
                      [--runtime <s>] [--note <s>]
    Create the profile if absent, else update in place. Only the flags passed
    change; everything else is preserved. Records origin = Agent or Human based
    on who runs it; writes as Draft.

firetrail profile component add <name> <path> [--summary <s>]
firetrail profile component rm  <name>
    Manage the shallow component map.
```

- **Trust transitions reuse `firetrail trust`** (Draft → Reviewed → Verified) — no
  bespoke review command. "Confirming the profile" is a trust transition like any
  other record.
- **Other crates read the profile via `Storage`/the index, not the CLI.** The
  audit loop (E) and `doctor` pull the `RepoProfile` record directly in Rust; the
  CLI is for the agent and humans.

### 3. The bootstrap skill (agent side)

`firetrail init` does **not** block on the profile — it sets up storage, hooks,
and writes the skill suite as it does today. The profile is populated by an
agent-run conversation that `doctor` and the AGENTS.md block nudge toward when
it's missing.

The extended `firetrail-bootstrap` skill instructs the agent to:

1. **Inspect** the repo — manifests (`Cargo.toml`, `package.json`, `Makefile`,
   `justfile`), CI configs (`.github/workflows`, etc.) for the real validate/test
   commands, and the directory layout for the component map. *Agent judgment, not
   firetrail code.*
2. **Propose** what it found and **discuss with the user**, with extra care on the
   validate command (that's what the audit loop runs).
3. **Persist incrementally** as the user confirms each piece, via `firetrail
   profile set` / `profile component add` (writes `Draft`, `origin = Agent`).
4. **Finalize** — once the user is happy, transition `Draft → Reviewed`/`Verified`
   via `firetrail trust`. That's the signal downstream tools trust.
5. **New repo** — little to detect, so the agent mostly asks; leaving
   `validate_command` empty initially is fine (`doctor` keeps reminding).

Re-running the skill later is just another `profile set` round: it updates the
existing record and re-enters `Draft` until re-confirmed — exactly the behavior
wanted when the build setup changes.

### 4. `doctor` check + validate-command policy

"Require a validate command if possible" is implemented as a **persistent nudge
with graceful degradation**, not a hard gate (a hard block would make firetrail
painful to adopt in a repo without a clean validate command yet).

`firetrail doctor` gains profile checks:

| Condition | Severity | Message |
|---|---|---|
| No `RepoProfile` record | **warn** | "No repo profile — run the firetrail-bootstrap skill." |
| Profile exists, `validate_command` empty | **warn** | "No validate command — the audit loop can't run a deterministic gate." |
| Profile still `Draft` (unconfirmed) | **info** | "Repo profile is unconfirmed — review and verify it." |
| Profile present + validate set + Reviewed/Verified | **ok** | — |

**Strict escape hatch for CI.** Interactive `firetrail doctor` never blocks. But
`firetrail doctor --strict` returns a **non-zero exit code** when
`validate_command` is empty or the profile is unverified (`Draft`). This lets a
team enforce "no merge without a confirmed validate command" in their pipeline
without adding friction to day-to-day local use. Default (non-`--strict`)
behavior stays exactly as the table above.

Downstream (forward-looking to E): the **audit loop degrades gracefully** — with a
validate command it runs the deterministic gate *plus* agent judgment; without
one it warns and falls back to agent judgment only. A missing validate command
weakens the audit but never bricks the workflow.

### 5. UI read/edit surface

ft-ui is Axum + a TS/SPA frontend with a per-domain route pattern. New routes:

```
GET    /api/profile                   → current RepoProfile (404 if none)
PUT    /api/profile                   → partial update (same fields as `profile set`)
POST   /api/profile/components        → add { name, path, summary? }
DELETE /api/profile/components/:name  → remove one
```

Confirmation (Draft → Reviewed → Verified) goes through the **existing
`/api/trust/*` routes**. Frontend: a **Profile panel** showing commands, tooling
facts, and component map (each editable inline) plus a trust badge + "Verify"
action wired to the existing trust UI. Same partial-update semantics as the CLI.

### External-mode mechanics (clean host repo)

In external storage mode firetrail clones the data (memory) repo into the host
working tree at `.firetrail/cache/data-repo/` (`CLONE_SUBPATH`), and `init` adds
`.firetrail/cache/` to `X`'s `.gitignore`. So `X`'s git never sees the clone —
committing in `X` won't complain. The `RepoProfile` record is written, committed,
and pushed to the data repo's own history, separate from `X`'s commits.

**Caveat to handle:** the data repo is cloned lazily on first access. Bootstrap
must ensure the clone exists before writing the profile (clone-on-demand at the
start of `profile set`).

## Testing & verification

Tests-first, at every layer, using `ft-testkit`'s `TestRepo`:

- **ft-core:** `RepoProfileBody` serde round-trip; `RecordKind::RepoProfile` ↔
  prefix mapping; `state_hash` excludes the hash fields.
- **ft-storage:** write/read; `ALL_KINDS` includes it (embedded + external);
  **singleton update semantics** — second `set` updates in place with a chained
  `prev_state_hash`, no duplicate.
- **ft-cli:** `profile set` partial-update preserves untouched fields;
  create-if-absent; `show --json` shape; `component add/rm`.
- **doctor:** correct severity per tier (no profile / no validate / Draft / ok).
- **ft-trust:** Draft → Reviewed transition applies to a profile record.
- **ft-ui:** `GET /api/profile` 404 when absent; `PUT` partial update; component
  add/delete.
- **External mode:** profile written into the cloned data-repo; host repo stays
  clean (clone gitignored); round-trips through the data repo's own git.

## Open questions / future extensions

- Per-scope profiles for monorepos (different validate commands per package).
- Whether `doctor`'s validate-command tier should ever escalate from `warn` to a
  blocking error in CI contexts (kept as `warn` for v1).

## Relationship to the larger initiative

| # | Sub-project | Depends on |
|---|---|---|
| **A** | **Repo profile + bootstrap + validate-command (this spec)** | — |
| B | Architecture docs + drift detection | A |
| C | Repo rules (`Rule` record + brainstorm flow) | A |
| D | Drift detection refinement | B |
| E | Audit loop (audits executor changes vs rules + validate cmd) | A, C |
| F | Rules/docs editing UI | C, B |

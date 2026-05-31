---
doc_type: reference
status: draft
links:
  - firetrail-lj41
---

# repo-profile — the `RepoProfile` record and its surfaces

**Epic:** `firetrail-lj41` (sub-project A)
**Crates touched:** ft-core, ft-storage, ft-trust, ft-cli, ft-ui
**Design spec:** `docs/specs/2026-05-31-repo-profile-bootstrap-design.md`

---

## Purpose

`RepoProfile` is a singleton-per-repo record holding the lightweight, always-read
facts firetrail needs about the host repo: the canonical validate command, the
standard test/build/lint commands, language/tooling facts, and a shallow
component map (names + paths only). The agent inspects the repo and **decides**
these contents; firetrail only **stores, indexes, validates, and surfaces** them
(ADR-0005). Firetrail ships no language/tooling auto-detection in Rust — that
judgment lives in the `firetrail-bootstrap` skill the agent follows.

This is the foundation for later sub-projects (architecture docs, repo rules, the
audit loop); see the spec's relationship table. Only the profile and its
bootstrap are in scope here.

---

## The record kind (`ft-core`)

A new `RecordKind::RepoProfile` was added the standard centralized way — the
synchronized edits across `ft-core`'s `id.rs` (prefix `PROFILE`, kind string
`repo_profile`) and `record.rs` (the `RecordBody::RepoProfile` variant), plus
`ft-storage`'s `ALL_KINDS` arrays for both backends. It reuses the existing
`RecordEnvelope` wholesale; only the body is new.

```rust
pub struct RepoProfileBody {
    validate_command: Option<String>,   // canonical "prove it's good" command; consumed by the audit loop
    test_command:     Option<String>,
    build_command:    Option<String>,
    lint_command:     Option<String>,   // formatting lives inside validate/lint — no separate format command
    languages:        Vec<String>,      // e.g. ["rust", "typescript"]
    package_managers: Vec<String>,      // e.g. ["cargo", "pnpm"]
    runtime:          Option<String>,   // e.g. "node 20"
    components:       Vec<ComponentRef>, // shallow map; { name, path, summary? }
    notes:            Option<String>,
    trust:            TrustState,        // Draft → Reviewed → Verified
}

pub struct ComponentRef { name: String, path: String, summary: Option<String> }
```

Every body field carries `#[serde(default)]`, so older payloads deserialize
without loss. `RecordBuilder::repo_profile(body)` constructs the record.

---

## Storage helper (`ft-storage`)

`RepoProfile` is a singleton by convention — one record per repo — so it gets two
free helpers in `ft-storage/src/profile.rs` that work against any backend
(embedded or external) through the `Storage` trait:

| Function | Purpose |
|---|---|
| `profile_get(storage) -> Option<Record>` | Read the current profile, or `None` if absent. If more than one is found (a degenerate state `doctor` warns about), returns the lexicographically smallest id so the result is deterministic. |
| `profile_set(storage, body, author) -> Record` | Upsert the singleton: update the existing record body **in place** (preserving `id`/`created_by`/`created_at`, bumping `updated_at`) if present, else create one authored by `author`. Recomputes `state_hash`; leaves `prev_state_hash` to `ft-history`. |

Records land under `records/repo_profile/<id>.json`.

---

## Trust lifecycle (`ft-trust`)

The existing trust state machine does the propose→confirm work for free, so there
is no bespoke review command. The agent writes the profile as `Draft`
(`origin = Agent`) — its proposal. A human confirming (in discussion or the UI)
transitions it `Draft → Reviewed → Verified` through the existing `ft-trust`
machinery. Downstream tools trust a `Reviewed`/`Verified` profile.

---

## CLI surface (`firetrail profile`)

The API the bootstrap skill calls. Partial-update throughout, so the agent fills
the profile in incrementally as the discussion unfolds. Other crates read the
profile via `profile_get`, not the CLI.

| Command | Purpose |
|---|---|
| `firetrail profile show [--json]` | Print the current profile; `--json` for agent/machine consumption. Exits non-zero if no profile exists. |
| `firetrail profile set [--validate <cmd>] [--test <cmd>] [--build <cmd>] [--lint <cmd>] [--language <lang>]… [--package-manager <pm>]… [--runtime <s>] [--note <s>]` | Create if absent, else update in place. Only the flags passed change; everything else is preserved. Records `origin` per who runs it; writes as `Draft`. |
| `firetrail profile component add <name> <path> [--summary <s>]` | Add an entry to the shallow component map. |
| `firetrail profile component rm <name>` | Remove a component-map entry. |

Trust transitions reuse `firetrail trust` — confirming the profile is a trust
transition like any other record.

The profile is populated by an agent-run conversation (the extended
`firetrail-bootstrap` skill), never by firetrail-side detection. `firetrail init`
does not block on the profile; `doctor` and the AGENTS.md block nudge toward it
when it is missing. Re-running the skill is just another `profile set` round: it
updates the existing record and re-enters `Draft` until re-confirmed.

---

## `doctor` check + validate-command policy

`firetrail doctor` gains profile checks — a persistent nudge with graceful
degradation, not a hard gate:

| Condition | Severity | Message |
|---|---|---|
| No `RepoProfile` record | warn | "No repo profile — run the firetrail-bootstrap skill." |
| Profile exists, `validate_command` empty | warn | "No validate command — the audit loop can't run a deterministic gate." |
| Profile still `Draft` (unconfirmed) | info | "Repo profile is unconfirmed — review and verify it." |
| Profile present + validate set + Reviewed/Verified | ok | — |

Interactive `firetrail doctor` never blocks. `firetrail doctor --strict` returns a
non-zero exit code when `validate_command` is empty or the profile is unverified
(`Draft`), letting a team enforce "no merge without a confirmed validate command"
in CI without adding friction to local use.

---

## UI surface (`ft-ui`)

ft-ui (Axum + a TS/SPA frontend) gains per-domain profile routes:

| Route | Purpose |
|---|---|
| `GET /api/profile` | Current `RepoProfile` (404 if none). |
| `PUT /api/profile` | Partial update — same fields as `profile set`. |
| `POST /api/profile/components` | Add `{ name, path, summary? }`. |
| `DELETE /api/profile/components/:name` | Remove one component-map entry. |

Confirmation (`Draft → Reviewed → Verified`) goes through the existing
`/api/trust/*` routes. The frontend Profile panel shows commands, tooling facts,
and the component map (each editable inline) plus a trust badge and "Verify"
action wired to the existing trust UI.

---

## External-mode mechanics

In external storage mode firetrail clones the data repo into the host working
tree under `.firetrail/cache/data-repo/`, and `init` adds `.firetrail/cache/` to
the host repo's `.gitignore`, so the host repo's git never sees the clone. The
`RepoProfile` record is written, committed, and pushed to the data repo's own
history. The clone is lazy, so `profile set` clones-on-demand before writing.

---

## Out of scope (future sub-projects)

These build on the profile and are documented in their own specs:

- B — Architecture docs + drift detection
- C — Repo rules (`Rule` record + brainstorm flow)
- D — Drift detection refinement
- E — Audit loop (consumes `validate_command`; degrades gracefully when absent)
- F — Rules/docs editing UI

Per-scope profiles for monorepos (different validate commands per package) are a
future extension; v1 is one profile per repo.

---

## References

- ADR-0005 — Firetrail does not call LLMs at runtime (agent decides, firetrail stores)
- ADR-0006 — Storage modes (external-mode mechanics)
- ADR-0013 — Trust model (Draft → Reviewed → Verified)
- ADR-0017 — Audit chain integrity (`state_hash` on write)
- Design spec — `docs/specs/2026-05-31-repo-profile-bootstrap-design.md`
- `docs/components/ft-core.md`, `docs/components/ft-storage.md`
</content>
</invoke>

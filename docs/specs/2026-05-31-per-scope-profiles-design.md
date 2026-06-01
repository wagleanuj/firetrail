---
doc_type: design
status: draft
scope: ft-cli
links:
  - firetrail-jr02
  - firetrail-lj41
supersedes_note: >
  Replaces the "Per-scope profiles for monorepos" future-extension note in
  docs/specs/2026-05-31-repo-profile-bootstrap-design.md.
---

# Per-Scope Repo Profiles — Design

> **Status:** Draft — awaiting review
> **Sub-project A.2** — extends the repo profile (Sub-project A) to monorepos.

## Context

Sub-project A shipped a **singleton** `RepoProfile`: one record per repo holding
the validate/test/build/lint commands, language/tooling facts, and a shallow
component map. It explicitly deferred *per-scope profiles for monorepos with
different validate commands per package* as YAGNI. This spec un-defers that.

The requirement is a **single model that serves both standalone and monorepo
repos**:

- A standalone repo keeps today's behavior with **zero overhead** — it never
  learns that per-scope profiles exist.
- A monorepo can give each package its own validate/test/build/lint commands
  (and tooling facts), so the audit loop (Sub-project E) can run *the right*
  command for a given change.

This is a direct application of **ADR-0004 (multi-scope records)** — every record
already carries `owning_scope`/`affected_scopes` on its envelope — and
**ADR-0005 (no LLM in the tool)** — the agent decides per-package commands; firetrail
stores, resolves, and validates them.

### Prior-art grounding

The design follows the convergent consensus of the monorepo/config ecosystem,
with two choices made deliberately against a naive first instinct:

- **Base + sparse per-package delta, member-wins inheritance.** Deno
  (*"package takes priority over workspace"*) and Jest (root config copied down
  into each project) are the model. Vitest's *no-inheritance* `projects` is the
  anti-pattern — it forces every package to re-declare everything.
- **2-level cascade only** (base + one scope). Deep nesting is where every tool
  accumulates bugs (Cargo nested workspaces, Nx `extends` shallow-merge).
- **List fields replace, they do not union.** tsconfig `include`/`exclude`,
  Biome inner arrays, and Cargo all replace; array-merging is a documented
  footgun (TypeScript #57486). Semantically, `languages: ["rust"]` on a scope
  means *"this package is Rust,"* not *"base languages plus Rust"* — union would
  make it impossible to **narrow** a scope.
- **Path → profile resolution is last-declared-wins**, identical to CODEOWNERS
  (which firetrail already parses with last-match-wins). ESLint deliberately
  removed filesystem-cascade / most-specific resolution in its flat-config
  migration for exactly this reason: specificity scoring is non-local, ambiguous
  for overlapping globs, and nobody else computes it. Using last-declared gives
  firetrail **one precedence model end-to-end**.
- **Central storage keyed by scope id**, not co-located per-package files. This
  is the Renovate `packageRules` / CODEOWNERS shape for *operational* metadata.
  Because profiles key to **scope id** and only `scopes.yaml` touches raw path
  globs, a directory move updates one glob in one place and every profile stays
  intact — refactor resilience achieved centrally, dodging both the
  central-registry path-drift problem and the co-located discovery footgun.

## Goals

- One repo-wide **base** profile (today's singleton) plus **optional** per-scope
  profiles that inherit from it and override only what differs.
- **Path-driven resolution:** given a changed file, resolve the validate command
  that proves *that* change is good.
- **Changeset resolution:** given a set of changed files (a PR), resolve the
  distinct set of validate commands to run.
- **Explicit per-scope lookup:** read/edit a named scope's profile from CLI + UI.
- **Zero behavioral change** for repos without `.firetrail/scopes.yaml`.
- New `doctor` coverage checks that exploit central queryability.

## Non-goals

- More than 2 cascade levels (no scope-within-scope nesting). A leaf scope that
  needs a third level fully specifies its own fields. (Future extension.)
- A build-style dependency graph for transitive "affected" detection (Nx/Turbo
  style). `validate_plan` resolves only the *directly* touched scopes. (Future
  extension; firetrail's record dep-graph is unrelated to build impact.)
- Co-located per-package profile files. (Possible future hybrid; see below.)
- An array-extend escape hatch (Turborepo `$TURBO_EXTENDS$` style). Lists replace
  in v1; the sentinel is added only if a real need appears.

## Design

### 1. Data model — no new record, no new key

A per-scope profile is **a `RepoProfile` record whose `envelope.owning_scope =
Some("apps/checkout")`**. The base is the record with `owning_scope = None`.
`RepoProfileBody` is **unchanged**.

```
.firetrail/records/   (or the external memory repo)
  repo-profile-aaaa.json   owning_scope: null              ← base (today's singleton)
  repo-profile-bbbb.json   owning_scope: "apps/checkout"   ← overrides only test_command
  repo-profile-cccc.json   owning_scope: "libs/ui"         ← overrides lint_command + components
```

**One record per scope** (firetrail's existing JSON-per-record grain) — never a
monolithic `profiles.yaml`. This keeps concurrent edits to different scopes
conflict-free, avoiding the central-file write-contention failure mode.

A scope profile is a **sparse delta**: it sets only the fields that differ from
base. A scope record that overrides nothing but is bound to a scope id is legal
and means "identical to base."

The singleton invariant relaxes from *"≤1 `RepoProfile` total"* to:

> **≤1 base (`owning_scope: None`) and ≤1 profile per distinct `owning_scope`.**

### 2. Resolution

Two pure functions in a new `ft-ops::profile::resolve` module.

**Merge (base ⊕ delta), member-wins, field-level.**

```rust
fn merge(base: &RepoProfileBody, scope: &RepoProfileBody) -> RepoProfileBody {
    RepoProfileBody {
        // scalars: delta wins if set, else inherit base
        validate_command: scope.validate_command.clone().or_else(|| base.validate_command.clone()),
        test_command:     scope.test_command.clone().or_else(|| base.test_command.clone()),
        build_command:    scope.build_command.clone().or_else(|| base.build_command.clone()),
        lint_command:     scope.lint_command.clone().or_else(|| base.lint_command.clone()),
        runtime:          scope.runtime.clone().or_else(|| base.runtime.clone()),
        notes:            scope.notes.clone().or_else(|| base.notes.clone()),
        // lists: replace if the delta sets a non-empty value, else inherit base
        languages:        if scope.languages.is_empty()        { base.languages.clone() }        else { scope.languages.clone() },
        package_managers: if scope.package_managers.is_empty() { base.package_managers.clone() } else { scope.package_managers.clone() },
        components:       if scope.components.is_empty()        { base.components.clone() }       else { scope.components.clone() },
        // trust is per-record: the override carries its own propose→confirm lifecycle
        trust:            scope.trust,
    }
}
```

> An empty list cannot be distinguished from "unset" in the current body (both
> serialize to nothing). That is acceptable: a scope that genuinely wants *no*
> languages is degenerate, and "empty ⇒ inherit" is the least-surprising rule.

**Path → scope, last-declared-wins.** `scopes_for_path()` already returns all
matching scopes in `scopes.yaml` source order. The resolver takes the **last**
one (highest precedence), exactly mirroring `ft-scope`'s CODEOWNERS semantics.

```rust
fn scope_for_path<'a>(reg: &'a ScopeRegistry, path: &Path) -> Option<&'a Scope> {
    reg.scopes_for_path(path).into_iter().last()   // last-declared wins
}

pub fn resolve_for_path(storage, reg, path) -> RepoProfileBody {
    let base = profile_get_base(storage)?.map(body).unwrap_or_default();
    match scope_for_path(reg, path) {
        Some(scope) => match profile_get_for_scope(storage, &scope.id)? {
            Some(delta) => merge(&base, &body(delta)),
            None        => base,            // scope exists but no profile → base
        },
        None => base,                       // no scope matches → base
    }
}
```

Authoring convention (documented, CODEOWNERS-identical): **broad patterns first,
narrow exceptions last; a catch-all goes at the top.** `doctor` warns when a
broad pattern is declared *last* and would shadow narrower ones (the classic
CODEOWNERS footgun).

### 3. Changeset → validate plan (the audit-loop entry point)

```rust
pub struct ValidateEntry { pub command: String, pub scopes: Vec<String>, pub file_count: usize }
pub struct ValidatePlan  { pub entries: Vec<ValidateEntry>, pub unresolved: usize /* files with no validate cmd */ }

pub fn validate_plan(storage, reg, changed_paths: &[PathBuf]) -> ValidatePlan;
```

For each changed path: resolve its merged profile → its `validate_command`.
Collect the **distinct** commands (a `BTreeMap<command, ValidateEntry>` keyed on
the command string), remembering which scopes/how many files demanded each, so
the agent/CI can report *why* each command runs. Run each distinct command once;
the audit passes iff all pass. Paths whose resolved profile has no
`validate_command` increment `unresolved` (surfaced, not silently dropped).

### 4. CLI surface — `--scope` everywhere, two new verbs

Omitting `--scope` operates on the base — byte-identical to today.

```
firetrail profile show     [--scope <id>] [--resolved] [--json]
    --resolved prints the merged effective profile for <id> (base ⊕ delta).
firetrail profile set       --scope <id>  [--validate <cmd>] [--test <cmd>] [--build <cmd>]
                            [--lint <cmd>] [--language <l>]... [--package-manager <pm>]...
                            [--runtime <s>] [--note <s>]
firetrail profile component add/rm  --scope <id> ...

firetrail profile list                         # NEW: base + every scope profile, one row each
firetrail profile resolve  --paths <a> <b> …   # NEW: validate plan for an explicit changeset
firetrail profile resolve  --staged            # NEW: convenience — resolve git's staged diff
firetrail profile resolve  --base <ref>        # NEW: convenience — resolve diff vs <ref>
    All resolve forms accept --json.
```

`profile set --scope X` **errors if `X` is not a scope in `scopes.yaml`** — no
orphan profiles. Trust transitions stay on `firetrail trust` (per-record).

### 5. `doctor` checks (free wins from central queryability)

| Condition | Severity |
|---|---|
| `owning_scope` names a scope absent from `scopes.yaml` (dangling profile) | **warn** |
| Two profiles share the same `owning_scope` (or two bases) | **warn** |
| A scope in `scopes.yaml` has an `applies_to` glob matching **zero** files | **warn** |
| A broad pattern declared *last* shadows narrower scopes | **warn** |
| `--strict`: base has no `validate_command` **and** some enabled scope also resolves to none | **warn → non-zero** |

The existing base-profile checks (no profile / no validate / Draft / ok) are
unchanged. Coverage reporting ("scopes with no profile", "files matched by no
scope") is available via `profile list` / `resolve` rather than a doctor gate.

### 6. UI

The Profile panel gains a **scope switcher** (Base · apps/checkout · libs/ui …)
and a **"Resolved" toggle** that shows the merged effective profile for the
selected scope. Routes extend with a query param rather than new shapes:

```
GET /api/profile?scope=<id>             → that scope's stored delta (404 if none)
GET /api/profile?scope=<id>&resolved=1  → merged base ⊕ delta
PUT /api/profile?scope=<id>             → partial update of the scope delta
GET /api/profile/resolve?paths=a,b,…    → ValidatePlan (JSON)
```

Trust confirmation continues through the existing `/api/trust/*` routes.

### 7. Storage accessors

`ft-storage::profile` gains scope-aware accessors; the current singleton becomes
the base accessor:

```rust
pub fn profile_get_base(storage)                  -> Result<Option<Record>, _>; // owning_scope == None
pub fn profile_get_for_scope(storage, scope_id)   -> Result<Option<Record>, _>; // owning_scope == Some(id)
pub fn profile_list(storage)                       -> Result<Vec<Record>, _>;     // base + all scopes
pub fn profile_set_for_scope(storage, scope_id, body, author) -> Result<Record, _>;
```

`profile_get` (no scope) keeps returning the base, so existing callers are
unaffected. Upsert keys on `(kind == RepoProfile, owning_scope)`.

## Standalone / monorepo unification — the zero-overhead guarantee

- **No `scopes.yaml` ⇒ `scopes_for_path` returns empty ⇒ every path resolves to
  base.** Identical to today. The words "scope", "delta", "inheritance" never
  surface for a standalone user.
- The base record schema is **unchanged** — adding scopes never requires editing
  the existing base ("grow in place", à la Cargo's optional `[workspace]`).
- Per-scope concepts live only behind `--scope` / the scope switcher; the
  standalone quickstart and docs never mention them.

**Acceptance gate:** a repo with no `scopes.yaml` produces byte-identical
`profile show`, `doctor`, and prime output before and after this feature ships.

## Testing & verification

Tests-first at every layer, via `ft-testkit`'s `TestRepo`:

- **ft-ops resolve:** `merge` field-fallthrough (each scalar inherits when unset,
  overrides when set); list replace-if-non-empty-else-inherit; `trust` is the
  delta's own. `scope_for_path` last-declared-wins on overlapping globs.
  `validate_plan` de-dupes commands, counts files/scopes per command, counts
  `unresolved`.
- **ft-storage:** `profile_get_base` ignores scope records; `profile_get_for_scope`
  matches on `owning_scope`; upsert keys on `(kind, owning_scope)` (no duplicate
  per scope); `profile_get` (legacy) still returns base.
- **ft-cli:** `profile set --scope` errors on unknown scope; `show --scope
  --resolved` prints merged; `list` shows base + scopes; `resolve --paths`/`--staged`
  JSON shape and dedup; **zero-overhead**: a no-`scopes.yaml` repo is unchanged.
- **doctor:** dangling-scope, duplicate-per-scope, zero-match-glob,
  broad-last-shadow, and `--strict` coverage exit code.
- **ft-ui:** `?scope=` 404 when absent; `?resolved=1` merge; `PUT ?scope=` partial
  update; `/api/profile/resolve` plan.

## Open questions / future extensions

- Transitive "affected" resolution via a build dep-graph (Nx/Turbo style).
- An explicit array-extend sentinel if replace-only proves too blunt for `languages`.
- A hybrid co-located override file ingested into the central store, if individual
  teams ever need to own their package's profile without touching central records.
- Deeper than 2-level cascade (scope groups).

## Relationship to the larger initiative

This is **Sub-project A.2**. It does not change A's record schema, trust
lifecycle, or bootstrap skill (the skill simply learns to pass `--scope` when it
detects a monorepo). It strengthens the foundation that B (architecture docs),
C (rules), and E (audit loop) build on: work, docs, rules, and profiles all bind
to a package through the *same* `owning_scope` axis, so a future
`firetrail scope show <id>` can render the whole per-package bundle.

The scope axis itself — `.firetrail/scopes.yaml`, its last-declared-wins
resolution, and the authoring surface this design's `--scope` flags target — is
documented in `docs/components/scope-authoring.md` and ADR-0004's authoring
addendum (`docs/decisions/0004-multi-scope-records.md`). Per-scope profiles are
the first consumer of that axis beyond CODEOWNERS routing.

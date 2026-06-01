# Per-Scope Repo Profiles Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend the singleton `RepoProfile` into a base + optional per-scope deltas model so monorepos resolve per-package validate/test/build/lint commands while standalone repos keep zero-overhead behavior.

**Architecture:** A per-scope profile is a `RepoProfile` record with `envelope.owning_scope = Some(scope_id)`; the base is `owning_scope = None`. Resolution merges base ⊕ scope-delta (member-wins, lists replace-if-present); path→scope is last-declared-wins (CODEOWNERS-consistent, via `ft_scope::ScopeRegistry::scopes_for_path`). Storage stays central, one JSON record per scope.

**Tech Stack:** Rust 2024 (workspace crates ft-storage, ft-ops, ft-cli, ft-ui), `ft-scope` (glob registry), Axum + TS/SPA frontend, `ft-testkit::TestRepo` fixtures.

**Spec:** `docs/specs/2026-05-31-per-scope-profiles-design.md` · **Epic:** `firetrail-jr02`

**Phase dependency graph:**
```
P1 ft-storage ──▶ P2 ft-ops resolve ──▶ P3 ft-cli surface
                                    ├──▶ P4 ft-cli doctor   (P3 ∥ P4)
                                    └──▶ P5 ft-ui
```

**Quality gates (run after each phase):** `just lint` (clippy -D warnings), `cargo fmt --check`, `cargo test -p <crate>`; full `just` before final validation.

---

## File Structure

| File | Responsibility | Change |
|---|---|---|
| `crates/ft-storage/src/profile.rs` | base/scope-aware singleton accessors | modify |
| `crates/ft-ops/src/profile/resolve.rs` | pure merge + path→scope + validate_plan | **create** |
| `crates/ft-ops/src/profile/mod.rs` | re-export resolve; scope-aware ops variants | modify |
| `crates/ft-cli/src/cli.rs` | `--scope` flags, `ProfileListArgs`, `ProfileResolveArgs` | modify |
| `crates/ft-cli/src/commands/profile.rs` | scope-aware show/set/component, list, resolve | modify |
| `crates/ft-cli/src/commands/doctor.rs` | per-scope coverage checks | modify |
| `crates/ft-ui/src/.../profile` routes + `web/` Profile panel | `?scope=`/`?resolved=`, resolve route, scope switcher | modify |
| `crates/ft-cli/tests/profile_and_doctor.rs` | integration coverage | modify |

---

## Phase 1 — ft-storage: base/scope-aware accessors

### Task 1.1: `profile_get_base` + `profile_get_for_scope`

**Files:**
- Modify: `crates/ft-storage/src/profile.rs`
- Test: same file `#[cfg(test)] mod tests`

- [ ] **Step 1: Write failing tests**

Add to the tests module (reuse existing `open`, `sample_body`, `make_identity`). Note: `RecordBuilder` has `.owning_scope(s)`; build scope records directly and `storage.write` them.

```rust
fn scope_record(scope: &str, validate: &str) -> Record {
    let mut body = RepoProfileBody::default();
    body.validate_command = Some(validate.into());
    RecordBuilder::new(RecordKind::RepoProfile, "Repo profile", make_identity())
        .owning_scope(scope)
        .repo_profile(body)
        .build()
        .unwrap()
}

#[test]
fn base_get_ignores_scope_records() {
    let tr = TestRepo::new().unwrap();
    let s = open(&tr);
    profile_set(&s, sample_body(), &make_identity()).unwrap(); // base
    s.write(&scope_record("apps/checkout", "pnpm test")).unwrap();

    let base = profile_get_base(&s).unwrap().expect("base present");
    assert_eq!(base.envelope.owning_scope, None);
}

#[test]
fn scope_get_matches_owning_scope() {
    let tr = TestRepo::new().unwrap();
    let s = open(&tr);
    s.write(&scope_record("apps/checkout", "pnpm test")).unwrap();
    s.write(&scope_record("libs/ui", "pnpm --filter ui test")).unwrap();

    let got = profile_get_for_scope(&s, "apps/checkout").unwrap().expect("present");
    assert_eq!(got.envelope.owning_scope.as_deref(), Some("apps/checkout"));
    assert!(profile_get_for_scope(&s, "nope").unwrap().is_none());
}
```

- [ ] **Step 2: Run, verify fail** — `cargo test -p ft-storage profile::tests::base_get` → FAIL (unresolved fn).

- [ ] **Step 3: Implement** in `profile.rs` (above the tests module). Partition in Rust — do **not** rely on `StorageFilter::scope` (it also matches `affected_scopes`):

```rust
/// Read the **base** repo profile (`owning_scope == None`), or `None`.
///
/// In a monorepo the base is the repo-wide profile that per-scope profiles
/// inherit from. Deterministic on the degenerate >1-base state: smallest id.
///
/// # Errors
/// Any error from [`Storage::list`] / [`Storage::read`].
pub fn profile_get_base(storage: &dyn Storage) -> Result<Option<Record>, StorageError> {
    let mut bases: Vec<Record> = profile_records(storage)?
        .into_iter()
        .filter(|r| r.envelope.owning_scope.is_none())
        .collect();
    bases.sort_by(|a, b| a.envelope.id.as_str().cmp(b.envelope.id.as_str()));
    Ok(bases.into_iter().next())
}

/// Read the per-scope profile delta for `scope_id` (`owning_scope == Some`), or
/// `None`. Deterministic on duplicates: smallest id.
///
/// # Errors
/// Any error from [`Storage::list`] / [`Storage::read`].
pub fn profile_get_for_scope(
    storage: &dyn Storage,
    scope_id: &str,
) -> Result<Option<Record>, StorageError> {
    let mut hits: Vec<Record> = profile_records(storage)?
        .into_iter()
        .filter(|r| r.envelope.owning_scope.as_deref() == Some(scope_id))
        .collect();
    hits.sort_by(|a, b| a.envelope.id.as_str().cmp(b.envelope.id.as_str()));
    Ok(hits.into_iter().next())
}

/// Read every `RepoProfile` record (base + all scopes), id-sorted.
fn profile_records(storage: &dyn Storage) -> Result<Vec<Record>, StorageError> {
    let mut ids = storage.list(&StorageFilter::default().kind(RecordKind::RepoProfile))?;
    ids.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    ids.into_iter().map(|id| storage.read(&id)).collect()
}
```

- [ ] **Step 4: Run, verify pass** — `cargo test -p ft-storage profile::tests` → PASS.

- [ ] **Step 5: Commit** — `git commit -am "feat(ft-storage): profile_get_base + profile_get_for_scope (firetrail-jr02)"`

### Task 1.2: `profile_list` + `profile_set_for_scope`

**Files:** Modify + test `crates/ft-storage/src/profile.rs`

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn list_returns_base_and_scopes() {
    let tr = TestRepo::new().unwrap();
    let s = open(&tr);
    profile_set(&s, sample_body(), &make_identity()).unwrap();
    s.write(&scope_record("apps/checkout", "pnpm test")).unwrap();
    let all = profile_list(&s).unwrap();
    assert_eq!(all.len(), 2);
}

#[test]
fn set_for_scope_upserts_in_place() {
    let tr = TestRepo::new().unwrap();
    let s = open(&tr);
    let mut b = RepoProfileBody::default();
    b.test_command = Some("pnpm test".into());
    let first = profile_set_for_scope(&s, "apps/checkout", b.clone(), &make_identity()).unwrap();
    assert_eq!(first.envelope.owning_scope.as_deref(), Some("apps/checkout"));

    b.test_command = Some("pnpm --filter checkout test".into());
    let second = profile_set_for_scope(&s, "apps/checkout", b, &make_identity()).unwrap();
    assert_eq!(first.envelope.id, second.envelope.id, "upsert in place");
    assert_eq!(profile_get_for_scope(&s, "apps/checkout").unwrap().unwrap().envelope.id, first.envelope.id);
    // base untouched / absent
    assert!(profile_get_base(&s).unwrap().is_none());
}
```

- [ ] **Step 2: Run, verify fail.**

- [ ] **Step 3: Implement** (mirror `profile_set`, keyed on scope):

```rust
/// Every `RepoProfile` record (base + per-scope), id-sorted.
///
/// # Errors
/// Any error from [`Storage::list`] / [`Storage::read`].
pub fn profile_list(storage: &dyn Storage) -> Result<Vec<Record>, StorageError> {
    profile_records(storage)
}

/// Upsert the per-scope profile delta for `scope_id` in place; create with
/// `owning_scope = Some(scope_id)` if absent. Mirrors [`profile_set`].
///
/// # Errors
/// Any error from [`Storage::list`] / [`Storage::read`] / [`Storage::write`],
/// or [`StorageError::Core`] on first build.
pub fn profile_set_for_scope(
    storage: &dyn Storage,
    scope_id: &str,
    body: RepoProfileBody,
    author: &Identity,
) -> Result<Record, StorageError> {
    if let Some(mut existing) = profile_get_for_scope(storage, scope_id)? {
        existing.body = RecordBody::RepoProfile(body);
        existing.envelope.updated_at = Utc::now();
        existing.envelope.state_hash = String::new();
        existing.envelope.state_hash = state_hash(&existing)?;
        storage.write(&existing)?;
        Ok(existing)
    } else {
        let record = RecordBuilder::new(RecordKind::RepoProfile, PROFILE_TITLE, author.clone())
            .owning_scope(scope_id)
            .repo_profile(body)
            .build()?;
        storage.write(&record)?;
        Ok(record)
    }
}
```

> Update the existing module-doc line "A repo has at most one `RepoProfile`" to note the new invariant: at most one base + at most one per `owning_scope`.

- [ ] **Step 4: Run, verify pass.**
- [ ] **Step 5: Commit** — `feat(ft-storage): profile_list + profile_set_for_scope (firetrail-jr02)`

---

## Phase 2 — ft-ops: the resolve module (the heart)

### Task 2.1: `merge` — base ⊕ delta

**Files:**
- Create: `crates/ft-ops/src/profile/resolve.rs`
- Modify: `crates/ft-ops/src/profile/mod.rs` (add `pub mod resolve;` near top)

- [ ] **Step 1: Failing test** in `resolve.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ft_core::RepoProfileBody;

    fn base() -> RepoProfileBody {
        let mut b = RepoProfileBody::default();
        b.validate_command = Some("just ci".into());
        b.test_command = Some("cargo test".into());
        b.languages = vec!["rust".into()];
        b
    }

    #[test]
    fn delta_overrides_scalar_inherits_rest() {
        let mut delta = RepoProfileBody::default();
        delta.test_command = Some("pnpm test".into());
        let m = merge(&base(), &delta);
        assert_eq!(m.validate_command.as_deref(), Some("just ci"));   // inherited
        assert_eq!(m.test_command.as_deref(), Some("pnpm test"));      // overridden
    }

    #[test]
    fn nonempty_list_replaces_empty_inherits() {
        let mut delta = RepoProfileBody::default();
        delta.languages = vec!["typescript".into()];
        assert_eq!(merge(&base(), &delta).languages, vec!["typescript".to_string()]);

        let empty = RepoProfileBody::default(); // languages empty
        assert_eq!(merge(&base(), &empty).languages, vec!["rust".to_string()]);
    }
}
```

- [ ] **Step 2: Run, verify fail** — `cargo test -p ft-ops resolve::tests` → FAIL.

- [ ] **Step 3: Implement** the module head + `merge`:

```rust
//! Pure resolution for per-scope repo profiles: `merge` (base ⊕ delta),
//! `scope_for_path` (last-declared-wins), and `validate_plan` (changeset →
//! distinct validate commands). No storage/IO here — callers pass bodies in.
//!
//! Design: `docs/specs/2026-05-31-per-scope-profiles-design.md`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ft_core::RepoProfileBody;
use ft_scope::ScopeRegistry;

/// Merge a per-scope delta over the base profile, member-wins.
///
/// Scalar fields take the delta when `Some`, else inherit base. List fields
/// (`languages`, `package_managers`, `components`) replace base when the delta's
/// list is non-empty, else inherit. `trust` is the delta's own (per-record).
#[must_use]
pub fn merge(base: &RepoProfileBody, delta: &RepoProfileBody) -> RepoProfileBody {
    RepoProfileBody {
        validate_command: delta.validate_command.clone().or_else(|| base.validate_command.clone()),
        test_command: delta.test_command.clone().or_else(|| base.test_command.clone()),
        build_command: delta.build_command.clone().or_else(|| base.build_command.clone()),
        lint_command: delta.lint_command.clone().or_else(|| base.lint_command.clone()),
        runtime: delta.runtime.clone().or_else(|| base.runtime.clone()),
        notes: delta.notes.clone().or_else(|| base.notes.clone()),
        languages: pick_list(&base.languages, &delta.languages),
        package_managers: pick_list(&base.package_managers, &delta.package_managers),
        components: pick_list(&base.components, &delta.components),
        trust: delta.trust,
    }
}

fn pick_list<T: Clone>(base: &[T], delta: &[T]) -> Vec<T> {
    if delta.is_empty() { base.to_vec() } else { delta.to_vec() }
}
```

- [ ] **Step 4: Run, verify pass.**
- [ ] **Step 5: Commit** — `feat(ft-ops): profile merge (base ⊕ delta) (firetrail-jr02)`

### Task 2.2: `scope_for_path` — last-declared-wins

**Files:** Modify `crates/ft-ops/src/profile/resolve.rs`

- [ ] **Step 1: Failing test.** Build a `ScopeRegistry` from YAML via the testkit pattern (write `.firetrail/scopes.yaml` under a `TestRepo` and `ScopeRegistry::load(root)`):

```rust
#[test]
fn last_declared_scope_wins() {
    use ft_testkit::TestRepo;
    let tr = TestRepo::new().unwrap();
    std::fs::create_dir_all(tr.root().join(".firetrail")).unwrap();
    std::fs::write(
        tr.root().join(".firetrail/scopes.yaml"),
        "scopes:\n  - id: all\n    applies_to: [\"**\"]\n  - id: checkout\n    applies_to: [\"apps/checkout/**\"]\n",
    ).unwrap();
    let reg = ScopeRegistry::load(tr.root()).unwrap();

    let id = scope_for_path(&reg, Path::new("apps/checkout/main.ts")).map(|s| s.id.clone());
    assert_eq!(id.as_deref(), Some("checkout")); // last-declared of the two matches
    let id2 = scope_for_path(&reg, Path::new("README.md")).map(|s| s.id.clone());
    assert_eq!(id2.as_deref(), Some("all"));
}
```

- [ ] **Step 2: Run, verify fail.**
- [ ] **Step 3: Implement:**

```rust
/// The scope governing `path`, last-declared-wins (mirrors CODEOWNERS / the
/// `ft-scope` source order). `None` when no scope matches.
#[must_use]
pub fn scope_for_path<'a>(reg: &'a ScopeRegistry, path: &Path) -> Option<&'a ft_scope::Scope> {
    reg.scopes_for_path(path).into_iter().last()
}
```

- [ ] **Step 4: Run, verify pass.**
- [ ] **Step 5: Commit** — `feat(ft-ops): scope_for_path last-declared-wins (firetrail-jr02)`

### Task 2.3: `validate_plan` — changeset → distinct commands

**Files:** Modify `crates/ft-ops/src/profile/resolve.rs`

- [ ] **Step 1: Failing test.** A resolver takes `(base, scope_deltas_by_id, registry, paths)`. To keep the function pure/testable, pass a closure that yields a scope's delta:

```rust
#[test]
fn plan_dedupes_and_counts() {
    use ft_testkit::TestRepo;
    let tr = TestRepo::new().unwrap();
    std::fs::create_dir_all(tr.root().join(".firetrail")).unwrap();
    std::fs::write(
        tr.root().join(".firetrail/scopes.yaml"),
        "scopes:\n  - id: checkout\n    applies_to: [\"apps/checkout/**\"]\n",
    ).unwrap();
    let reg = ScopeRegistry::load(tr.root()).unwrap();

    let mut base = RepoProfileBody::default();
    base.validate_command = Some("just ci".into());
    let mut checkout = RepoProfileBody::default();
    checkout.validate_command = Some("pnpm --filter checkout test".into());

    let paths = vec![
        PathBuf::from("apps/checkout/a.ts"),
        PathBuf::from("apps/checkout/b.ts"),
        PathBuf::from("README.md"),
    ];
    let plan = validate_plan(&reg, &base, &paths, |id| {
        if id == "checkout" { Some(checkout.clone()) } else { None }
    });
    // two distinct commands: checkout's (2 files) + base's (1 file)
    assert_eq!(plan.entries.len(), 2);
    let checkout_entry = plan.entries.iter().find(|e| e.command.contains("checkout")).unwrap();
    assert_eq!(checkout_entry.file_count, 2);
    assert_eq!(checkout_entry.scopes, vec!["checkout".to_string()]);
    assert_eq!(plan.unresolved, 0);
}
```

- [ ] **Step 2: Run, verify fail.**
- [ ] **Step 3: Implement:**

```rust
/// One distinct validate command in a [`ValidatePlan`], with provenance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidateEntry {
    /// The validate command to run.
    pub command: String,
    /// Scope ids (sorted, unique) that resolved to this command. Empty = base.
    pub scopes: Vec<String>,
    /// How many changed files resolved to this command.
    pub file_count: usize,
}

/// The set of distinct validate commands a changeset requires.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ValidatePlan {
    /// Distinct commands, ordered by command string.
    pub entries: Vec<ValidateEntry>,
    /// Changed files whose resolved profile has no validate command.
    pub unresolved: usize,
}

/// Resolve a changeset to the distinct validate commands to run. `scope_delta`
/// yields a scope's stored delta body (or `None`); the caller wires it to
/// `ft_storage::profile_get_for_scope`.
pub fn validate_plan(
    reg: &ScopeRegistry,
    base: &RepoProfileBody,
    paths: &[PathBuf],
    mut scope_delta: impl FnMut(&str) -> Option<RepoProfileBody>,
) -> ValidatePlan {
    // command -> (set of scope ids, file count)
    let mut acc: BTreeMap<String, (std::collections::BTreeSet<String>, usize)> = BTreeMap::new();
    let mut unresolved = 0usize;
    for path in paths {
        let (resolved, scope_id) = match scope_for_path(reg, path) {
            Some(scope) => match scope_delta(&scope.id) {
                Some(delta) => (merge(base, &delta), Some(scope.id.clone())),
                None => (base.clone(), Some(scope.id.clone())),
            },
            None => (base.clone(), None),
        };
        match resolved.validate_command {
            Some(cmd) => {
                let slot = acc.entry(cmd).or_default();
                if let Some(id) = scope_id { slot.0.insert(id); }
                slot.1 += 1;
            }
            None => unresolved += 1,
        }
    }
    let entries = acc.into_iter().map(|(command, (scopes, file_count))| ValidateEntry {
        command,
        scopes: scopes.into_iter().collect(),
        file_count,
    }).collect();
    ValidatePlan { entries, unresolved }
}
```

- [ ] **Step 4: Run, verify pass** — `cargo test -p ft-ops resolve` → PASS. Run `just lint` (clippy clean).
- [ ] **Step 5: Commit** — `feat(ft-ops): validate_plan changeset resolver (firetrail-jr02)`

---

## Phase 3 — ft-cli: `--scope`, `profile list`, `profile resolve`

> Depends on P1+P2. Parallelizable with P4.

### Task 3.1: `--scope` on show/set/component (+ unknown-scope error)

**Files:** Modify `crates/ft-cli/src/cli.rs`, `crates/ft-cli/src/commands/profile.rs`, test `crates/ft-cli/tests/profile_and_doctor.rs`

- [ ] **Step 1: Failing integration test** (follow the existing harness in `profile_and_doctor.rs` — it shells out to the built binary or calls command fns; match the file's existing style). Assert:
  - `profile set --scope apps/checkout --test "pnpm test"` writes a record with `owning_scope=apps/checkout` **only when** the scope exists in `scopes.yaml`;
  - `profile set --scope nope ...` exits non-zero (`unknown scope`);
  - `profile show --scope apps/checkout` prints the stored delta; `--resolved` prints base ⊕ delta;
  - a repo with **no** `scopes.yaml` and no `--scope` behaves byte-identically to today (zero-overhead guard).

- [ ] **Step 2: Run, verify fail.**

- [ ] **Step 3: Implement.** In `cli.rs`: add `--scope <ID>` (`pub scope: Option<String>`) to `ProfileShowArgs`, `ProfileSetArgs`, `ProfileComponentAddArgs`, `ProfileComponentRmArgs`; add `--resolved` (`pub resolved: bool`) to `ProfileShowArgs`. In `profile.rs`:
  - Add a helper `fn require_scope(ctx, command, scope_id) -> Result<(), CliError>` that loads `ScopeRegistry::load(&ctx.ws.root)` and errors `CliError::user` if `registry.get(scope_id).is_none()`.
  - Branch each command on `args.scope`: when `Some(id)`, validate via `require_scope`, then use `profile_get_for_scope` / `profile_set_for_scope` (add a scope-aware `persist`); when `None`, keep today's base path.
  - For `show --resolved` with a scope: load base via `profile_get_base`, delta via `profile_get_for_scope`, render `ft_ops::profile::resolve::merge(&base_body, &delta_body)`.

- [ ] **Step 4: Run, verify pass.**
- [ ] **Step 5: Commit** — `feat(ft-cli): --scope on profile show/set/component (firetrail-jr02)`

### Task 3.2: `firetrail profile list`

**Files:** `cli.rs` (`ProfileCmd::List`), `commands/profile.rs`, test file.

- [ ] **Step 1: Failing test** — `profile list` shows the base + one row per scope profile (id, scope, validate present?), `--json` is an array.
- [ ] **Step 2: Run, verify fail.**
- [ ] **Step 3: Implement** — add `List(ProfileListArgs)` variant + a `list()` fn calling `ft_storage::profile_list`, rendering one row per record (scope = `owning_scope.unwrap_or("(base)")`).
- [ ] **Step 4: Verify pass.**
- [ ] **Step 5: Commit** — `feat(ft-cli): firetrail profile list (firetrail-jr02)`

### Task 3.3: `firetrail profile resolve --paths/--staged/--base`

**Files:** `cli.rs` (`ProfileResolveArgs`), `commands/profile.rs`, test file. Reuse `ft-git` for the staged/base diff (grep `crates/ft-git` for the changed-paths helper used by `check pr`).

- [ ] **Step 1: Failing test** — `profile resolve --paths apps/checkout/a.ts README.md --json` returns a `ValidatePlan` with the expected distinct entries (use a `scopes.yaml` + a base + a scope delta set up in the test).
- [ ] **Step 2: Run, verify fail.**
- [ ] **Step 3: Implement** — `ProfileResolveArgs { paths: Vec<PathBuf>, staged: bool, base: Option<String> }` (paths xor staged xor base). Gather paths (explicit, or via ft-git diff). Load `ScopeRegistry`, `profile_get_base` body (or default), call `resolve::validate_plan(&reg, &base, &paths, |id| profile_get_for_scope(&ctx.storage, id).ok().flatten().and_then(body))`. Render entries (command, scopes, file_count) + `unresolved` count; `--json` serializes the plan.
- [ ] **Step 4: Verify pass + `just lint`.**
- [ ] **Step 5: Commit** — `feat(ft-cli): firetrail profile resolve (validate plan) (firetrail-jr02)`

---

## Phase 4 — ft-cli doctor: per-scope coverage checks

> Depends on P1+P2. Parallelizable with P3.

**Files:** Modify `crates/ft-cli/src/commands/doctor.rs`, test `crates/ft-cli/tests/profile_and_doctor.rs`

### Task 4.1: dangling-scope + duplicate-per-scope + zero-match-glob + broad-last-shadow

- [ ] **Step 1: Failing tests** — set up a `scopes.yaml` + profile records and assert `doctor` emits, at the right severity:
  - `profile.scope.dangling` (warn) when a profile's `owning_scope` is not in `scopes.yaml`;
  - `profile.scope.duplicate` (warn) when two records share an `owning_scope`;
  - `scope.glob.empty` (warn) when an `applies_to` glob matches zero files in the repo;
  - `scope.order.shadow` (warn) when a broad pattern is declared last and shadows a narrower earlier scope.
- [ ] **Step 2: Run, verify fail.**
- [ ] **Step 3: Implement** — extend `check_profile` (or add `check_profile_scopes`) using `ft_storage::profile_list` + `ScopeRegistry::load`. For zero-match: walk tracked files (reuse the repo file-walk doctor already uses, or `ft-git` ls-files) and test each scope's matchers. For broad-last-shadow: a scope whose patterns match a strict superset of a strictly-earlier scope's matched files, declared after it. Append `CheckResult`s matching the existing helper signature.
- [ ] **Step 4: Verify pass.**
- [ ] **Step 5: Commit** — `feat(ft-cli): doctor per-scope profile coverage checks (firetrail-jr02)`

### Task 4.2: `--strict` coverage gate

- [ ] **Step 1: Failing test** — base has no `validate_command` **and** an enabled scope also resolves to none ⇒ `doctor --strict` exits non-zero; with base validate set ⇒ exits zero.
- [ ] **Step 2: Run, verify fail.**
- [ ] **Step 3: Implement** — in the strict-violations path, after existing base checks, for each enabled scope (`registry.is_scope_enabled`) compute `merge(base, delta).validate_command`; push a violation when `None`.
- [ ] **Step 4: Verify pass + `just lint`.**
- [ ] **Step 5: Commit** — `feat(ft-cli): doctor --strict per-scope validate coverage (firetrail-jr02)`

---

## Phase 5 — ft-ui: scope-aware routes + Profile panel switcher

> Depends on P1+P2.

### Task 5.1: ops scope-aware variants

**Files:** Modify `crates/ft-ops/src/profile/mod.rs`

- [ ] **Step 1: Failing tests** (ops-level, `TestRepo` + `Workspace`): a `get_for_scope(ws, id, resolved)` returns the stored delta when `resolved=false` and `merge(base, delta)` when `resolved=true`; an `update` variant accepts an optional `scope` and writes via `profile_set_for_scope`.
- [ ] **Step 2: Run, verify fail.**
- [ ] **Step 3: Implement** — add `scope: Option<String>` + `resolved: bool` plumbing. Generalize `with_profile` to take an optional scope (drive `profile_get_for_scope`/`profile_set_for_scope` when `Some`, else base). Add `get` variant returning the merged view when `resolved`. Keep existing `get`/`update` signatures working (base path) to avoid breaking callers — add new fns or default the new params.
- [ ] **Step 4: Verify pass.**
- [ ] **Step 5: Commit** — `feat(ft-ops): scope-aware profile get/update + resolved view (firetrail-jr02)`

### Task 5.2: routes `?scope=` / `?resolved=` + `/api/profile/resolve`

**Files:** Modify the ft-ui profile route module (grep `crates/ft-ui/src` for `/api/profile`). Test: the crate's route tests.

- [ ] **Step 1: Failing tests** — `GET /api/profile?scope=apps/checkout` 404 when absent; `&resolved=1` returns merged; `PUT /api/profile?scope=...` partial-updates the delta; `GET /api/profile/resolve?paths=a,b` returns a `ValidatePlan` JSON.
- [ ] **Step 2: Run, verify fail.**
- [ ] **Step 3: Implement** — add `scope`/`resolved` query extractors to the existing handlers, route to the new ops variants; add the `resolve` handler. Regenerate ts-rs bindings if the build does so (run the crate's binding-gen step).
- [ ] **Step 4: Verify pass.**
- [ ] **Step 5: Commit** — `feat(ft-ui): /api/profile scope+resolved params + resolve route (firetrail-jr02)`

### Task 5.3: Profile panel scope switcher + Resolved toggle

**Files:** Modify `crates/ft-ui/web/` Profile panel component + its tests.

- [ ] **Step 1: Failing test** — frontend test: the panel renders a scope switcher (Base + scopes from `profile list`/a scopes endpoint), switching refetches `?scope=`, the Resolved toggle refetches `?resolved=1`.
- [ ] **Step 2: Run, verify fail** (`pnpm test` / the web test runner).
- [ ] **Step 3: Implement** — add the switcher + toggle, wire to the new query params, show a per-scope trust badge. Follow the existing panel's data-fetching pattern.
- [ ] **Step 4: Verify pass** — web typecheck + tests.
- [ ] **Step 5: Commit** — `feat(ft-ui): Profile panel scope switcher + resolved toggle (firetrail-jr02)`

---

## Final validation (whole batch)

- [ ] `just` (build + test + clippy -D warnings + fmt-check) green across the workspace.
- [ ] ft-ui frontend: typecheck + tests green; ts-rs bindings regenerated and committed if changed.
- [ ] **Zero-overhead regression:** in a `TestRepo` with no `scopes.yaml`, `profile show` / `doctor` output is unchanged from `main` (diff against a baseline capture).
- [ ] Spec coverage sweep (a verification subagent): every spec §1–§7 requirement maps to a landed task; resolution is last-declared-wins; lists replace; base = `owning_scope:None`.
- [ ] Close `firetrail-jr02` children; update the epic.

---

## Spec-coverage self-review (author pass)

| Spec section | Task(s) |
|---|---|
| §1 data model (owning_scope key, one record/scope, relaxed singleton) | 1.1, 1.2 |
| §2 merge (member-wins, list replace) | 2.1 |
| §2 path→scope (last-declared) | 2.2 |
| §3 validate_plan (dedupe, unresolved) | 2.3, 3.3 |
| §4 CLI (--scope, list, resolve, unknown-scope error) | 3.1, 3.2, 3.3 |
| §5 doctor checks (dangling/dup/zero-glob/shadow/strict) | 4.1, 4.2 |
| §6 UI (?scope, ?resolved, resolve route, switcher) | 5.1, 5.2, 5.3 |
| §7 storage accessors | 1.1, 1.2 |
| zero-overhead guarantee | 3.1 test, final validation |

No placeholders; types (`merge`, `scope_for_path`, `validate_plan`, `ValidatePlan`/`ValidateEntry`, `profile_get_base`/`profile_get_for_scope`/`profile_list`/`profile_set_for_scope`) are consistent across phases.

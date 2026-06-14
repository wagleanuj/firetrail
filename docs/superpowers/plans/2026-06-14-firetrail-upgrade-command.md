# `firetrail upgrade` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `firetrail upgrade` (and `firetrail upgrade --check`) subcommand that self-updates the installed binary to the latest GitHub release via the `axoupdater` library.

**Architecture:** A new `commands::upgrade` module exposes `run()` returning the existing `CommandOutcome`/`CliError` contract. A pure, serializable `UpgradeOutcome` carries the result and renders markdown / quiet / JSON; the networked axoupdater interaction is isolated in `run()`. The command does not require a workspace.

**Tech Stack:** Rust, clap (derive), `axoupdater` 0.10 (blocking API), serde.

**Tracking:** bd `firetrail-7oic`. Spec: `docs/superpowers/specs/2026-06-14-firetrail-upgrade-command-design.md`.

---

## File Structure

| File | Responsibility |
|------|----------------|
| `crates/ft-cli/Cargo.toml` | Add the `axoupdater` dependency (blocking feature). |
| `crates/ft-cli/src/commands/upgrade.rs` | **New.** `UpgradeArgs` handling via `run()`, `UpgradeOutcome` struct + rendering, axoupdater orchestration, unit tests. |
| `crates/ft-cli/src/cli.rs` | Add `Upgrade(UpgradeArgs)` to `Command`; define `UpgradeArgs`. |
| `crates/ft-cli/src/commands/mod.rs` | Add `pub mod upgrade;`, `CommandOutcome::Upgrade`, and the arms in `command()`, `markdown()`, `quiet_line()`, `json_data()`, `warnings()`. |
| `crates/ft-cli/src/main.rs` | Dispatch `Command::Upgrade`. |
| `docs/USER_GUIDE.md` | Document the command. |

---

## Task 1: Add the `axoupdater` dependency

**Files:**
- Modify: `crates/ft-cli/Cargo.toml`

- [ ] **Step 1: Add the dependency**

In `crates/ft-cli/Cargo.toml`, under `[dependencies]` (after the `anyhow` / `clap` lines near the top of that section), add:

```toml
# Self-update for the `upgrade` command. Blocking API (no async main in ft-cli).
axoupdater = { version = "0.10", features = ["blocking"] }
```

- [ ] **Step 2: Fetch & compile the dependency**

Run: `cargo build -p ft-cli`
Expected: compiles successfully (downloads axoupdater + transitive deps on first run). If the build fails because GitHub-release support is not in the default features, change the line to `features = ["blocking", "github_releases"]` and rebuild.

- [ ] **Step 3: Check licenses/advisories (cargo-deny is enforced in CI)**

Run: `cargo deny check 2>&1 | tail -20` (skip if `cargo-deny` is not installed locally)
Expected: no new `error`. If a transitive dependency trips a license/advisory rule, add a narrowly-scoped exception to `deny.toml` (e.g. an `[licenses] allow` entry or an `[advisories] ignore`) and note it in the commit message.

- [ ] **Step 4: Commit**

```bash
git add crates/ft-cli/Cargo.toml Cargo.lock deny.toml
git commit -m "build(ft-cli): add axoupdater dependency for self-update"
```

---

## Task 2: `UpgradeOutcome` struct and rendering (TDD)

This is the pure, hermetic core: construct outcomes and render them. No network.

**Files:**
- Create: `crates/ft-cli/src/commands/upgrade.rs`
- Test: inline `#[cfg(test)]` module in `crates/ft-cli/src/commands/upgrade.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/ft-cli/src/commands/upgrade.rs` with ONLY the outcome type, its constructors/renderers, and tests:

```rust
//! `firetrail upgrade` — self-update the installed binary to the latest
//! GitHub release via `axoupdater`.
//!
//! `upgrade` operates on the *tool*, not a workspace, so unlike most commands
//! it does not require an initialised Firetrail workspace. When the binary was
//! not installed by the Firetrail installer (no install receipt — e.g. a
//! `cargo install` or hand-copied build) it fails with an actionable message
//! rather than attempting a network update.

use serde::Serialize;

/// Outcome of `firetrail upgrade`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpgradeOutcome {
    /// Version of the running binary.
    pub current_version: String,
    /// Whether a newer release exists.
    pub update_available: bool,
    /// True only when an install actually ran this invocation.
    pub installed: bool,
    /// The version installed (only set when `installed` is true and known).
    pub new_version: Option<String>,
    /// True for `--check`: nothing was installed regardless of availability.
    pub checked_only: bool,
}

impl UpgradeOutcome {
    /// `--check` result: report availability, install nothing.
    #[must_use]
    pub fn checked(current_version: String, update_available: bool) -> Self {
        Self {
            current_version,
            update_available,
            installed: false,
            new_version: None,
            checked_only: true,
        }
    }

    /// Default-mode result when already on the latest release.
    #[must_use]
    pub fn up_to_date(current_version: String) -> Self {
        Self {
            current_version,
            update_available: false,
            installed: false,
            new_version: None,
            checked_only: false,
        }
    }

    /// Default-mode result after a successful install.
    #[must_use]
    pub fn upgraded(current_version: String, new_version: Option<String>) -> Self {
        Self {
            current_version,
            update_available: true,
            installed: true,
            new_version,
            checked_only: false,
        }
    }

    /// Markdown rendering (TTY default).
    #[must_use]
    pub fn markdown(&self) -> String {
        if self.installed {
            return match &self.new_version {
                Some(v) => format!(
                    "**upgrade** updated firetrail `{}` → `{}`\n",
                    self.current_version, v
                ),
                None => format!(
                    "**upgrade** updated firetrail from `{}` to the latest release\n",
                    self.current_version
                ),
            };
        }
        if self.checked_only && self.update_available {
            return format!(
                "**upgrade** a newer firetrail is available — you have `{}`. \
                 Run `firetrail upgrade` to install.\n",
                self.current_version
            );
        }
        format!(
            "**upgrade** firetrail is up to date (`{}`)\n",
            self.current_version
        )
    }

    /// One-line quiet summary.
    #[must_use]
    pub fn quiet_line(&self) -> String {
        if self.installed {
            return match &self.new_version {
                Some(v) => format!("upgrade {} -> {}", self.current_version, v),
                None => format!("upgrade {} -> latest", self.current_version),
            };
        }
        if self.checked_only && self.update_available {
            format!("upgrade {} (update available)", self.current_version)
        } else {
            format!("upgrade {} (up to date)", self.current_version)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checked_update_available_renders_call_to_action() {
        let o = UpgradeOutcome::checked("0.2.4".into(), true);
        assert!(o.checked_only);
        assert!(!o.installed);
        assert!(o.markdown().contains("newer firetrail is available"));
        assert!(o.markdown().contains("`0.2.4`"));
        assert_eq!(o.quiet_line(), "upgrade 0.2.4 (update available)");
    }

    #[test]
    fn checked_up_to_date_renders_up_to_date() {
        let o = UpgradeOutcome::checked("0.2.4".into(), false);
        assert!(o.markdown().contains("up to date"));
        assert_eq!(o.quiet_line(), "upgrade 0.2.4 (up to date)");
    }

    #[test]
    fn up_to_date_renders_up_to_date() {
        let o = UpgradeOutcome::up_to_date("0.2.4".into());
        assert!(!o.installed);
        assert!(o.markdown().contains("up to date"));
    }

    #[test]
    fn upgraded_renders_version_transition() {
        let o = UpgradeOutcome::upgraded("0.2.4".into(), Some("0.2.5".into()));
        assert!(o.installed);
        assert!(o.markdown().contains("`0.2.4` → `0.2.5`"));
        assert_eq!(o.quiet_line(), "upgrade 0.2.4 -> 0.2.5");
    }

    #[test]
    fn upgraded_without_known_version_still_renders() {
        let o = UpgradeOutcome::upgraded("0.2.4".into(), None);
        assert!(o.markdown().contains("to the latest release"));
        assert_eq!(o.quiet_line(), "upgrade 0.2.4 -> latest");
    }

    #[test]
    fn serializes_camel_case() {
        let v = serde_json::to_value(UpgradeOutcome::checked("0.2.4".into(), true)).unwrap();
        assert_eq!(v["currentVersion"], "0.2.4");
        assert_eq!(v["updateAvailable"], true);
        assert_eq!(v["checkedOnly"], true);
        assert_eq!(v["installed"], false);
    }
}
```

Also register the module so it compiles — add to `crates/ft-cli/src/commands/mod.rs` near the other `pub mod` declarations (alphabetical-ish, by the existing `pub mod ui;`):

```rust
pub mod upgrade;
```

- [ ] **Step 2: Run the tests to verify they pass (pure module, no wiring yet)**

Run: `cargo test -p ft-cli upgrade::tests 2>&1 | tail -20`
Expected: 6 tests pass. (They are self-contained — `UpgradeOutcome` has no external deps.)

> Note: this task's "test" is written to pass immediately because the type and its rendering are defined together; the TDD value is that the assertions pin the exact output strings the later wiring depends on. If you prefer a red step, comment out the method bodies, watch them fail to compile/assert, then restore.

- [ ] **Step 3: Commit**

```bash
git add crates/ft-cli/src/commands/upgrade.rs crates/ft-cli/src/commands/mod.rs
git commit -m "feat(ft-cli): UpgradeOutcome type + rendering for upgrade command"
```

---

## Task 3: Wire the command into the CLI surface

Add the clap args, the `CommandOutcome` variant + all dispatch arms, and the `main.rs` dispatch. After this the command parses and renders even before the network logic exists (Task 4 fills `run`).

**Files:**
- Modify: `crates/ft-cli/src/cli.rs`
- Modify: `crates/ft-cli/src/commands/mod.rs:109+` (enum + 5 match arms)
- Modify: `crates/ft-cli/src/main.rs`
- Modify: `crates/ft-cli/src/commands/upgrade.rs` (add a temporary `run`)

- [ ] **Step 1: Add `UpgradeArgs` and the `Command` variant in `cli.rs`**

Add the variant inside the `Command` enum (next to `Ui(UiArgs)` at `cli.rs:214`):

```rust
    /// Self-update the `firetrail` binary to the latest release.
    Upgrade(UpgradeArgs),
```

And define the args struct near `UiArgs` (after the `UiArgs` struct, ~`cli.rs:983`):

```rust
/// `firetrail upgrade [--check]` args.
#[derive(Debug, clap::Args)]
pub struct UpgradeArgs {
    /// Report whether a newer release exists without installing it.
    #[arg(long)]
    pub check: bool,
}
```

- [ ] **Step 2: Add a temporary `run` to `upgrade.rs` so the dispatch arm compiles**

Append to `crates/ft-cli/src/commands/upgrade.rs` (above the `#[cfg(test)]` module):

```rust
use crate::cli::{GlobalOpts, UpgradeArgs};
use crate::commands::CommandOutcome;
use crate::error::CliError;

/// `firetrail upgrade` entry point. (Network logic added in Task 4.)
pub fn run(args: &UpgradeArgs, _global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let current = env!("CARGO_PKG_VERSION").to_string();
    // Placeholder wiring — replaced in Task 4.
    let outcome = if args.check {
        UpgradeOutcome::checked(current, false)
    } else {
        UpgradeOutcome::up_to_date(current)
    };
    Ok(CommandOutcome::Upgrade(outcome))
}
```

- [ ] **Step 3: Add the `CommandOutcome::Upgrade` variant + dispatch arms in `mod.rs`**

In the `CommandOutcome` enum (after `Ui(ui::UiOutcome),` at `mod.rs:179`):

```rust
    /// `firetrail upgrade`.
    Upgrade(upgrade::UpgradeOutcome),
```

In `command()` (after the `Self::Ui(_) => "ui",` arm):

```rust
            Self::Upgrade(_) => "upgrade",
```

In `markdown()` (after the `Self::Daemon(d) => d.markdown(),` / `Self::Ui` group — add near the other simple arms):

```rust
            Self::Upgrade(u) => u.markdown(),
```

In `quiet_line()` (the dispatch mirrors `markdown()`; add the same arm):

```rust
            Self::Upgrade(u) => u.quiet_line(),
```

In `json_data()` (after `Self::Ui(u) => serde_json::to_value(u).unwrap_or(Value::Null),`):

```rust
            Self::Upgrade(u) => serde_json::to_value(u).unwrap_or(Value::Null),
```

In `warnings()` (add an arm returning no warnings):

```rust
            Self::Upgrade(_) => Vec::new(),
```

> If `markdown()` / `quiet_line()` / `warnings()` use a catch-all `_ =>` arm rather than exhaustive arms, add the explicit `Self::Upgrade(...)` arm before the catch-all instead.

- [ ] **Step 4: Dispatch in `main.rs`**

Add to the `match &cli.command { ... }` (next to `Command::Ui(args) => commands::ui::run(args, &cli.global),` at `main.rs:167`):

```rust
        Command::Upgrade(args) => commands::upgrade::run(args, &cli.global),
```

- [ ] **Step 5: Verify it compiles, parses, and renders**

Run: `cargo build -p ft-cli && ./target/debug/firetrail upgrade --help`
Expected: help shows `--check`. 

Run: `./target/debug/firetrail upgrade --check --json`
Expected: JSON envelope with `data.currentVersion` equal to the crate version and `data.checkedOnly: true` (placeholder reports up-to-date for now).

- [ ] **Step 6: Commit**

```bash
git add crates/ft-cli/src/cli.rs crates/ft-cli/src/commands/mod.rs crates/ft-cli/src/main.rs crates/ft-cli/src/commands/upgrade.rs
git commit -m "feat(ft-cli): wire upgrade command into CLI surface"
```

---

## Task 4: Implement the axoupdater orchestration (TDD for the no-receipt path)

Replace the placeholder `run` with the real logic, and add a hermetic test that a missing install receipt yields an actionable `CliError`.

**Files:**
- Modify: `crates/ft-cli/src/commands/upgrade.rs`

- [ ] **Step 1: Write the failing test (no-receipt → user error)**

Add to the `#[cfg(test)]` module in `upgrade.rs`:

```rust
    use crate::cli::{GlobalOpts, UpgradeArgs};

    /// In a clean HOME with no install receipt, `run` must fail with a clear,
    /// non-panicking user error rather than attempting a network update.
    #[test]
    fn run_without_receipt_is_a_friendly_user_error() {
        let tmp = tempfile::tempdir().unwrap();
        // Point every receipt-search root at an empty dir so no receipt is found.
        // SAFETY: single-threaded test; we set process env for the duration.
        unsafe {
            std::env::set_var("HOME", tmp.path());
            std::env::set_var("XDG_CONFIG_HOME", tmp.path());
            std::env::set_var("XDG_DATA_HOME", tmp.path());
        }
        let args = UpgradeArgs { check: true };
        let err = run(&args, &GlobalOpts::default())
            .expect_err("no receipt present, expected an error");
        let msg = format!("{err}");
        assert!(
            msg.contains("installer") || msg.contains("install"),
            "error should explain the install-method limitation, got: {msg}"
        );
    }
```

Notes for the implementer:
- `tempfile` is already a dev-dependency of the workspace (used widely in tests); if `cargo test -p ft-cli` reports it missing, add `tempfile = { workspace = true }` under `[dev-dependencies]` in `crates/ft-cli/Cargo.toml`.
- `GlobalOpts::default()` — if `GlobalOpts` does not derive `Default`, construct it explicitly with the fields visible at `cli.rs` (e.g. `GlobalOpts { format: None, json: false, quiet: false, verbose: false, workspace: None }`); match the actual field set.

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p ft-cli upgrade::tests::run_without_receipt_is_a_friendly_user_error 2>&1 | tail -20`
Expected: FAIL — the placeholder `run` returns `Ok(...)`, so `expect_err` panics.

- [ ] **Step 3: Implement the real `run`**

Replace the placeholder `run` in `upgrade.rs` with:

```rust
use axoupdater::AxoUpdater;
use crate::cli::{GlobalOpts, UpgradeArgs};
use crate::commands::CommandOutcome;
use crate::error::CliError;

/// `firetrail upgrade` entry point.
///
/// Self-updates via the install receipt the Firetrail installer wrote. Does
/// **not** require a workspace. Returns a user error (not a panic) when the
/// binary was installed some other way and has no receipt.
pub fn run(args: &UpgradeArgs, _global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let current = env!("CARGO_PKG_VERSION").to_string();

    let mut updater = AxoUpdater::new_for("firetrail");
    updater.load_receipt().map_err(|e| {
        CliError::user(
            "upgrade",
            format!(
                "this `firetrail` was not installed by the Firetrail installer, so it \
                 can't self-update ({e}). Re-run the install script from the latest \
                 GitHub release, or update via your package manager / `cargo install`."
            ),
        )
    })?;

    // Guard against updating a binary that the receipt isn't actually for
    // (e.g. a package-manager copy alongside a shell-installed receipt).
    if !updater
        .check_receipt_is_for_this_executable()
        .map_err(|e| CliError::internal("upgrade", e))?
    {
        return Err(CliError::user(
            "upgrade",
            "the running `firetrail` was not installed by the Firetrail installer \
             (the install receipt points at a different binary); update it the way \
             you installed it.",
        ));
    }

    let update_available = updater
        .is_update_needed_sync()
        .map_err(|e| CliError::internal("upgrade", e))?;

    if args.check {
        return Ok(CommandOutcome::Upgrade(UpgradeOutcome::checked(
            current,
            update_available,
        )));
    }

    if !update_available {
        return Ok(CommandOutcome::Upgrade(UpgradeOutcome::up_to_date(current)));
    }

    let result = updater
        .run_sync()
        .map_err(|e| CliError::internal("upgrade", e))?;
    let new_version = result.map(|r| r.new_version.to_string());
    Ok(CommandOutcome::Upgrade(UpgradeOutcome::upgraded(
        current,
        new_version,
    )))
}
```

Remove the now-duplicated `use crate::cli::...`, `use crate::commands::...`, `use crate::error::...` lines added in Task 3 Step 2 so the imports are declared once.

> API reference (axoupdater 0.10): `AxoUpdater::new_for(name)`, `load_receipt() -> AxoupdateResult<&mut AxoUpdater>`, `check_receipt_is_for_this_executable() -> AxoupdateResult<bool>`, `is_update_needed_sync() -> AxoupdateResult<bool>` (feature `blocking`), `run_sync() -> AxoupdateResult<Option<UpdateResult>>` (feature `blocking`); `UpdateResult.new_version` is a `semver::Version`. If a signature differs in the resolved version, run `cargo doc -p axoupdater --open` and adjust. If `check_receipt_is_for_this_executable` does not return `bool` in the resolved version, drop that guard block (the `load_receipt` check already covers the common no-receipt case).

- [ ] **Step 4: Run the no-receipt test to verify it passes**

Run: `cargo test -p ft-cli upgrade::tests::run_without_receipt_is_a_friendly_user_error 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 5: Run the full upgrade test module + clippy**

Run: `cargo test -p ft-cli upgrade:: 2>&1 | tail -20`
Expected: all upgrade tests pass.

Run: `cargo clippy -p ft-cli --all-targets -- -D warnings 2>&1 | tail -20`
Expected: no warnings (matches the pre-commit gate).

- [ ] **Step 6: Commit**

```bash
git add crates/ft-cli/src/commands/upgrade.rs crates/ft-cli/Cargo.toml Cargo.lock
git commit -m "feat(ft-cli): implement upgrade via axoupdater (load receipt, check, run)"
```

---

## Task 5: Document the command

**Files:**
- Modify: `docs/USER_GUIDE.md`

- [ ] **Step 1: Add a section**

Find the commands section of `docs/USER_GUIDE.md` and add:

```markdown
### Updating firetrail

Update an installed binary to the latest release:

    firetrail upgrade            # install the latest release
    firetrail upgrade --check    # report whether a newer release exists

`upgrade` only works for binaries installed via the Firetrail installer
(the `curl … | sh` script from a GitHub release), which records an install
receipt. For `cargo install` or hand-copied builds it prints how to update
instead. Note: `firetrail update <id>` is unrelated — it edits a record.
```

- [ ] **Step 2: Commit**

```bash
git add docs/USER_GUIDE.md
git commit -m "docs: document the firetrail upgrade command"
```

---

## Task 6: Final verification

- [ ] **Step 1: Full workspace gate**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -15`
Expected: clean (this is what the pre-commit hook runs).

Run: `cargo test -p ft-cli 2>&1 | tail -15`
Expected: all ft-cli tests pass.

- [ ] **Step 2: Smoke-test the binary**

Run: `./target/debug/firetrail upgrade --check` (outside any workspace, e.g. from `/tmp`)
Expected: a clean user error — the dev binary has no install receipt — with the "not installed by the Firetrail installer" guidance, exit code 1. This confirms the no-receipt path end-to-end.

- [ ] **Step 3: Close the issue**

The post-commit auto-close hook is not active in this clone; close manually:

```bash
bd close firetrail-7oic --reason "Shipped firetrail upgrade command"
```

(Real-world updating is verified manually after a release, by running `firetrail upgrade --check` on an installer-installed copy.)

---

## Self-review notes

- **Spec coverage:** command surface (`upgrade` + `--check`) → Task 3/4; no-workspace → Task 4 `run` (no `require_initialised`); friendly no-receipt error → Task 4 + test; JSON/quiet parity → Task 3 arms; outcome rendering tests → Task 2; manual post-release check → Task 6. All spec sections mapped.
- **Receipt emission:** confirmed — cargo-dist ≥ 0.9.0 shell/PowerShell installers write the receipt axoupdater reads; this repo uses dist 0.32, so no `dist-workspace.toml` change is required.
- **Runtime safety:** `ft-cli::main` is synchronous (`fn main() -> ExitCode`), so axoupdater's `*_sync` methods (which build their own current-thread tokio runtime) will not panic from a nested runtime.
- **Naming:** `upgrade` chosen to avoid the existing `update` (record edit) command.

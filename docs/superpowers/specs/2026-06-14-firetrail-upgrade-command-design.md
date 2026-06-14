# Design: `firetrail upgrade` self-update command

- **Date:** 2026-06-14
- **Status:** Approved (pending spec review)
- **Tracking:** _bd issue filed at plan time_

## Summary

Add a `firetrail upgrade` subcommand that self-updates the installed
`firetrail` binary to the latest GitHub release, using the
[`axoupdater`](https://github.com/axodotdev/axoupdater) library — the companion
to cargo-dist, which this project already uses to build and publish releases
(`dist-workspace.toml`, `installers = ["shell"]`).

The name `upgrade` is used (not `update`) because `firetrail update <id>`
already exists and updates a *record's* fields (`cli.rs`, `UpdateArgs`).

## Goals

- `firetrail upgrade` — check for and install the latest release in one step.
- `firetrail upgrade --check` — report whether a newer version exists, install
  nothing, exit 0.
- Behave like every other CLI command: honor `--format json` and `--quiet`.
- Run anywhere — `upgrade` operates on the tool, not on a workspace, so it must
  not require an initialised Firetrail workspace.
- Fail gracefully and actionably when the binary was not installed via the
  official installer (no install receipt).

## Non-goals

- No `--version <X>` pinning/downgrade (YAGNI; can be added later).
- No separate `firetrail-update` binary (that is the cargo-dist "Approach B"
  updater; we embed the library instead for a single-binary UX).
- No auto-update / background update checks.

## Architecture

New dependency: `axoupdater` in `crates/ft-cli/Cargo.toml`, using its blocking
API (the CLI dispatch path is synchronous). Confirm the exact feature flag
during the implementation plan's research step.

Wiring mirrors the existing command contract
(`Command::X(args) => commands::x::run(args, &cli.global)` returning
`Result<CommandOutcome, CliError>`):

| File | Change |
|------|--------|
| `crates/ft-cli/src/cli.rs` | Add `Upgrade(UpgradeArgs)` to the `Command` enum; `UpgradeArgs { check: bool }` exposing `--check`. |
| `crates/ft-cli/src/commands/upgrade.rs` | New module: `run(args, global) -> Result<CommandOutcome, CliError>`. |
| `crates/ft-cli/src/commands/mod.rs` | Add `CommandOutcome::Upgrade(UpgradeOutcome)`; implement `markdown()`, `quiet_line()`, and the `"upgrade"` arm of `command()`. |
| `crates/ft-cli/src/main.rs` | Dispatch arm `Command::Upgrade(args) => commands::upgrade::run(...)`. |

`UpgradeOutcome` is a serializable struct carrying the rendered result, e.g.:

```rust
pub struct UpgradeOutcome {
    pub current: String,
    pub latest: Option<String>,   // None when it could not be determined
    pub update_available: bool,
    pub installed: bool,          // true only when an install actually ran
    pub checked_only: bool,       // true for --check
}
```

## Behavior / data flow

1. Build `AxoUpdater::new_for("firetrail")` and `load_receipt()`.
2. **No receipt** (cargo-installed, hand-copied, or a dev build): return
   `CliError::user` with guidance — the binary was not installed via the
   Firetrail installer; update via `cargo install` or by re-running the install
   script. Never panic.
3. Query the latest release / whether an update is needed.
4. **`--check`:** populate `UpgradeOutcome` with `checked_only = true`,
   `installed = false`; print `current: X  latest: Y  (update available | up to
   date)`; exit 0 regardless of availability.
5. **default:** if up to date, report "already up to date"
   (`installed = false`); otherwise run the installer for the latest release and
   report the new version (`installed = true`).

`run` does **not** call `workspace::require_initialised`.

## Error handling

- Missing install receipt → `CliError::user` (actionable message).
- Network / GitHub API failure → `CliError` (external/internal) carrying the
  underlying axoupdater error message.

## Testing

- **Unit (no network):** `UpgradeOutcome` rendering — `markdown()`,
  `quiet_line()`, and JSON serialization for the up-to-date, update-available,
  and check-only cases.
- **Unit (no network):** the "missing receipt → friendly `CliError::user`" path,
  which fails locally without any network access.
- **Manual / post-release:** run `firetrail upgrade --check` against the real
  published release once v0.2.4+ artifacts exist.

axoupdater's networked install path is not unit-tested directly; the updater
interaction is kept behind a thin function so the outcome rendering and the
no-receipt path are the tested seams.

## Open research items (resolved in the implementation plan)

1. Confirm the dist **shell installer emits an install receipt** readable by
   axoupdater (default in recent dist). If a `dist-workspace.toml` toggle is
   required to emit the receipt, the plan adds it. No separate updater binary is
   shipped.
2. Confirm the correct `axoupdater` version and feature flag for a blocking,
   synchronous call from the CLI.

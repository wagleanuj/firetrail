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

use crate::cli::{GlobalOpts, UpgradeArgs};
use crate::commands::CommandOutcome;
use crate::error::CliError;
use axoupdater::AxoUpdater;

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

    // NOTE: the "no install receipt → friendly user error" path is verified by
    // the manual binary smoke test (`firetrail upgrade --check` on a build with
    // no receipt), not a unit test: triggering it deterministically requires
    // overriding HOME/XDG via `std::env::set_var`, which is `unsafe` in edition
    // 2024 and the workspace forbids `unsafe` (`unsafe_code = "forbid"`).
}

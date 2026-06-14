//! `firetrail upgrade` — self-update the installed binary to the latest
//! GitHub release via `axoupdater`.
//!
//! `upgrade` operates on the *tool*, not a workspace, so unlike most commands
//! it does not require an initialised Firetrail workspace. When the binary was
//! not installed by the Firetrail installer (no install receipt — e.g. a
//! `cargo install` or hand-copied build) it fails with an actionable message
//! rather than attempting a network update.

// The type below is wired into the CLI surface in a follow-up task; until then
// the non-test build has no caller, so allow dead code for this intermediate
// commit.
#![allow(dead_code)]

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

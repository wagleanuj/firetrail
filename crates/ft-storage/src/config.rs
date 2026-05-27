//! Workspace storage-mode configuration.
//!
//! Defines the [`StorageMode`] discriminator that selects between
//! [`crate::EmbeddedStorage`] (M1) and [`crate::ExternalStorage`] (M5), and the
//! [`ExternalConfig`] / [`SyncPolicy`] values surfaced to operators via
//! `.firetrail/config.yml`.
//!
//! The on-disk schema (intentionally a subset of the full Firetrail config
//! YAML) is:
//!
//! ```yaml
//! storage:
//!   mode: embedded        # or "external"
//!   data_repo_url: file:///path/to/data-repo.git   # external only
//!   default_branch: main                            # external, optional
//!   sync_policy: loose                              # external, optional
//! ```
//!
//! `open_for_workspace` reads the file, dispatches to the appropriate backend,
//! and returns a `Box<dyn Storage>` that downstream crates can hold as
//! `Arc<dyn Storage>`.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::external::{ExternalConfig, ExternalStorage, SyncPolicy};
use crate::{EmbeddedStorage, Storage, StorageError};

/// Discriminator for the active storage backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageMode {
    /// Records colocated with the working code repository.
    Embedded {
        /// Absolute path to the workspace (code-repo) root.
        root: PathBuf,
    },
    /// Records served from a separate data repository, cloned locally under
    /// `.firetrail/cache/data-repo`.
    External {
        /// Absolute path to the workspace (code-repo) root.
        workspace_root: PathBuf,
        /// URL of the remote data repository (file://, ssh, or https).
        data_repo_url: String,
        /// Resolved external-mode configuration.
        config: ExternalConfig,
    },
}

impl StorageMode {
    /// Read `.firetrail/config.yml` and decide which mode is active.
    ///
    /// # Errors
    ///
    /// - [`StorageError::NotInitialized`] if the config file is missing.
    /// - [`StorageError::Invalid`] if the YAML is malformed or the
    ///   `storage.mode` field is unrecognized.
    pub fn from_workspace(workspace_root: &Path) -> Result<Self, StorageError> {
        let config_path = workspace_root.join(".firetrail").join("config.yml");
        if !config_path.is_file() {
            return Err(StorageError::NotInitialized(config_path));
        }
        let raw = std::fs::read_to_string(&config_path)?;
        let parsed: WorkspaceConfigFile =
            serde_yaml::from_str(&raw).map_err(|e| StorageError::Invalid {
                path: config_path.clone(),
                reason: format!("yaml: {e}"),
            })?;
        let storage = parsed.storage.unwrap_or_default();
        let mode = storage.mode.as_deref().unwrap_or("embedded");
        match mode {
            "embedded" => Ok(Self::Embedded {
                root: workspace_root.to_path_buf(),
            }),
            "external" => {
                let data_repo_url = storage.data_repo_url.ok_or_else(|| StorageError::Invalid {
                    path: config_path.clone(),
                    reason: "storage.data_repo_url is required for external mode".into(),
                })?;
                let default_branch = storage.default_branch.unwrap_or_else(|| "main".to_string());
                let sync_policy = match storage.sync_policy.as_deref().unwrap_or("loose") {
                    "loose" => SyncPolicy::Loose,
                    other => {
                        return Err(StorageError::Invalid {
                            path: config_path,
                            reason: format!(
                                "storage.sync_policy '{other}' not supported at M5; \
                                 only 'loose' is implemented"
                            ),
                        });
                    }
                };
                let config = ExternalConfig {
                    data_repo_url: data_repo_url.clone(),
                    default_branch,
                    sync_policy,
                };
                Ok(Self::External {
                    workspace_root: workspace_root.to_path_buf(),
                    data_repo_url,
                    config,
                })
            }
            other => Err(StorageError::Invalid {
                path: config_path,
                reason: format!("storage.mode '{other}' is not recognized"),
            }),
        }
    }
}

/// Open the configured storage backend for a workspace.
///
/// Reads `.firetrail/config.yml` and constructs the appropriate
/// [`Storage`] implementation. Callers typically wrap the result in an
/// `Arc<dyn Storage>` and share it across threads.
///
/// # Errors
///
/// - [`StorageError::NotInitialized`] if the config file is missing.
/// - [`StorageError::Invalid`] if the YAML is malformed.
/// - Any error returned by the underlying backend's `open`.
pub fn open_for_workspace(workspace_root: &Path) -> Result<Box<dyn Storage>, StorageError> {
    let mode = StorageMode::from_workspace(workspace_root)?;
    match mode {
        StorageMode::Embedded { root } => {
            let s = EmbeddedStorage::open(root)?;
            Ok(Box::new(s))
        }
        StorageMode::External { config, .. } => {
            let s = ExternalStorage::open(workspace_root, &config)?;
            Ok(Box::new(s))
        }
    }
}

// ── On-disk YAML shape ──────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Default)]
struct WorkspaceConfigFile {
    storage: Option<StorageSection>,
}

#[derive(Debug, Deserialize, Default)]
struct StorageSection {
    mode: Option<String>,
    data_repo_url: Option<String>,
    default_branch: Option<String>,
    sync_policy: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use ft_testkit::TestRepo;

    fn write_config(tr: &TestRepo, yaml: &str) {
        let p = tr.root().join(".firetrail").join("config.yml");
        std::fs::write(p, yaml).unwrap();
    }

    #[test]
    fn from_workspace_defaults_to_embedded_when_storage_missing() {
        let tr = TestRepo::new().unwrap();
        write_config(&tr, "identity:\n  strict: false\n");
        let mode = StorageMode::from_workspace(tr.root()).unwrap();
        assert!(matches!(mode, StorageMode::Embedded { .. }));
    }

    #[test]
    fn from_workspace_dispatches_external_when_configured() {
        let tr = TestRepo::new().unwrap();
        write_config(
            &tr,
            "storage:\n  mode: external\n  data_repo_url: file:///tmp/foo\n",
        );
        let mode = StorageMode::from_workspace(tr.root()).unwrap();
        match mode {
            StorageMode::External {
                data_repo_url,
                config,
                ..
            } => {
                assert_eq!(data_repo_url, "file:///tmp/foo");
                assert_eq!(config.default_branch, "main");
                assert!(matches!(config.sync_policy, SyncPolicy::Loose));
            }
            other @ StorageMode::Embedded { .. } => {
                panic!("expected External, got {other:?}")
            }
        }
    }

    #[test]
    fn from_workspace_external_requires_url() {
        let tr = TestRepo::new().unwrap();
        write_config(&tr, "storage:\n  mode: external\n");
        let err = StorageMode::from_workspace(tr.root()).unwrap_err();
        assert!(matches!(err, StorageError::Invalid { .. }));
    }

    #[test]
    fn from_workspace_rejects_unknown_mode() {
        let tr = TestRepo::new().unwrap();
        write_config(&tr, "storage:\n  mode: martian\n");
        let err = StorageMode::from_workspace(tr.root()).unwrap_err();
        assert!(matches!(err, StorageError::Invalid { .. }));
    }

    #[test]
    fn missing_config_is_not_initialized() {
        let tr = TestRepo::new().unwrap();
        std::fs::remove_file(tr.root().join(".firetrail").join("config.yml")).ok();
        let err = StorageMode::from_workspace(tr.root()).unwrap_err();
        assert!(matches!(err, StorageError::NotInitialized(_)));
    }
}

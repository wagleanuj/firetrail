//! `firetrail doctor` — workspace health check.
//!
//! Each check runs independently and produces a [`CheckResult`]. Failures do
//! NOT stop the run — every check is executed so the user sees the full
//! picture in one pass. With `--fix`, safe remediations are attempted for
//! known-fixable failures (currently: rebuild the `SQLite` index).

use std::path::Path;

use ft_git::{HookName, Repo};
use ft_identity::{DefaultResolver, IdentityResolver};
use ft_index::Index;
use ft_storage::EmbeddedStorage;
use serde::Serialize;

/// Adapter that lets `ft_storage::EmbeddedStorage` satisfy
/// `ft_index::Storage`. The two traits will merge once `ft-storage` exports
/// the canonical trait; until then this glue lives here.
mod index_adapter {
    use std::path::{Path, PathBuf};

    use ft_core::{Record, RecordId};
    use ft_index::{
        Storage as IndexStorageTrait, StorageError as IndexStorageError, StorageFilter,
    };
    use ft_storage::{EmbeddedStorage, Storage as FsStorage};

    /// View of an [`EmbeddedStorage`] as an [`ft_index::Storage`].
    pub struct IndexStorage<'a> {
        inner: &'a EmbeddedStorage,
    }

    impl<'a> IndexStorage<'a> {
        /// Wrap an [`EmbeddedStorage`] reference.
        pub fn new(inner: &'a EmbeddedStorage) -> Self {
            Self { inner }
        }
    }

    fn map_err(e: ft_storage::StorageError) -> IndexStorageError {
        match e {
            ft_storage::StorageError::NotFound(id) => IndexStorageError::NotFound(id.to_string()),
            other => IndexStorageError::Other(other.to_string()),
        }
    }

    impl IndexStorageTrait for IndexStorage<'_> {
        fn iter(
            &self,
            filter: StorageFilter,
        ) -> Result<
            Box<dyn Iterator<Item = Result<(Record, PathBuf), IndexStorageError>> + '_>,
            IndexStorageError,
        > {
            // ft-storage's filter doesn't have a `include_closed` flag — its
            // default already returns every record. Pass-through `filter` is
            // accepted for forward compatibility and otherwise unused.
            let _ = filter;
            let fs_filter = ft_storage::StorageFilter::default();
            let ids = self.inner.list(&fs_filter).map_err(map_err)?;
            let inner = self.inner.clone();
            let iter = ids.into_iter().map(move |id| {
                let path = inner.path_for(&id);
                let record = inner.read(&id).map_err(map_err)?;
                Ok((record, path))
            });
            Ok(Box::new(iter))
        }

        fn read(&self, id: &RecordId) -> Result<(Record, PathBuf), IndexStorageError> {
            let path = self.inner.path_for(id);
            let record = self.inner.read(id).map_err(map_err)?;
            Ok((record, path))
        }

        fn read_path(&self, _path: &Path) -> Result<Record, IndexStorageError> {
            Err(IndexStorageError::Other(
                "read_path not implemented in CLI doctor adapter".into(),
            ))
        }
    }
}

use crate::cli::{DoctorArgs, GlobalOpts};
use crate::commands::CommandOutcome;
use crate::error::CliError;
use crate::workspace;

const COMMAND: &str = "doctor";

/// Severity of a single check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckStatus {
    /// Everything is in order.
    Ok,
    /// Not fatal, but worth noting.
    Warn,
    /// Failed; user action recommended.
    Fail,
}

/// One row in the doctor report.
#[derive(Debug, Clone, Serialize)]
pub struct CheckResult {
    /// Short, stable identifier for the check (e.g. `index.integrity`).
    pub id: String,
    /// Human-readable title.
    pub title: String,
    /// Outcome.
    pub status: CheckStatus,
    /// One-line summary.
    pub message: String,
    /// Suggested remedy (for `WARN` / `FAIL`).
    pub suggestion: Option<String>,
    /// Whether `--fix` attempted (and succeeded with) a remediation.
    pub fix_applied: bool,
}

impl CheckResult {
    fn ok(id: &str, title: &str, message: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            status: CheckStatus::Ok,
            message: message.into(),
            suggestion: None,
            fix_applied: false,
        }
    }

    fn warn(id: &str, title: &str, message: impl Into<String>, suggestion: &str) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            status: CheckStatus::Warn,
            message: message.into(),
            suggestion: Some(suggestion.into()),
            fix_applied: false,
        }
    }

    fn fail(id: &str, title: &str, message: impl Into<String>, suggestion: &str) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            status: CheckStatus::Fail,
            message: message.into(),
            suggestion: Some(suggestion.into()),
            fix_applied: false,
        }
    }
}

/// Aggregate report.
#[derive(Debug, Clone, Serialize)]
pub struct DoctorReport {
    /// Repo root that was checked.
    pub root: String,
    /// Whether all checks were `OK`.
    pub clean: bool,
    /// Individual checks, in display order.
    pub checks: Vec<CheckResult>,
    /// Non-fatal warnings outside the check list.
    pub warnings: Vec<String>,
}

impl DoctorReport {
    /// Markdown rendering.
    #[must_use]
    pub fn markdown(&self) -> String {
        use std::fmt::Write as _;
        let mut s = format!(
            "# firetrail doctor\n\nWorkspace: `{}`\nStatus: **{}**\n\n",
            self.root,
            if self.clean { "OK" } else { "needs attention" }
        );
        for c in &self.checks {
            let tag = match c.status {
                CheckStatus::Ok => "OK  ",
                CheckStatus::Warn => "WARN",
                CheckStatus::Fail => "FAIL",
            };
            let _ = writeln!(s, "- `{tag}` **{}** — {}", c.title, c.message);
            if let Some(sg) = &c.suggestion {
                if c.fix_applied {
                    let _ = writeln!(s, "    fix: {sg} (applied)");
                } else {
                    let _ = writeln!(s, "    fix: {sg}");
                }
            }
        }
        s
    }

    /// One-line summary for `--quiet`.
    #[must_use]
    pub fn quiet_line(&self) -> String {
        let (mut ok, mut warn, mut fail) = (0u32, 0u32, 0u32);
        for c in &self.checks {
            match c.status {
                CheckStatus::Ok => ok += 1,
                CheckStatus::Warn => warn += 1,
                CheckStatus::Fail => fail += 1,
            }
        }
        format!("doctor: {ok} ok, {warn} warn, {fail} fail")
    }
}

/// Entry point.
pub fn run(args: &DoctorArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let ws = workspace::locate(COMMAND, global.workspace.as_deref())?;
    let mut checks = Vec::new();
    let mut warnings = Vec::new();

    if args.network {
        warnings.push(
            "--network checks become meaningful in later milestones; currently a no-op".into(),
        );
    }

    // Workspace presence.
    if ws.firetrail_dir().is_dir() {
        checks.push(CheckResult::ok(
            "workspace.present",
            "Workspace directory",
            format!("{} exists", ws.firetrail_dir().display()),
        ));
    } else {
        checks.push(CheckResult::fail(
            "workspace.present",
            "Workspace directory",
            ".firetrail/ is missing",
            "run `firetrail init`",
        ));
    }

    // Config validity.
    check_config(&ws, &mut checks);

    // Identity resolution.
    check_identity(&ws.root, &mut checks);

    // Git status.
    check_git(&ws.root, &mut checks);

    // Hooks.
    check_hooks(&ws.root, &mut checks);

    // Index integrity (+ optional fix).
    check_index(&ws, args.fix, &mut checks);

    // Storage / index record count parity.
    check_storage_parity(&ws, &mut checks);

    let clean = checks.iter().all(|c| c.status == CheckStatus::Ok);
    let _ = global;

    Ok(CommandOutcome::Doctor(DoctorReport {
        root: ws.root.display().to_string(),
        clean,
        checks,
        warnings,
    }))
}

fn check_config(ws: &workspace::Workspace, checks: &mut Vec<CheckResult>) {
    let path = ws.config_path();
    if !path.exists() {
        checks.push(CheckResult::fail(
            "config.present",
            "Config file",
            format!("{} missing", path.display()),
            "run `firetrail init`",
        ));
        return;
    }
    match std::fs::read_to_string(&path) {
        Ok(text) => match serde_yaml::from_str::<serde_yaml::Value>(&text) {
            Ok(_) => checks.push(CheckResult::ok(
                "config.present",
                "Config file",
                ".firetrail/config.yml parses cleanly",
            )),
            Err(e) => checks.push(CheckResult::fail(
                "config.parse",
                "Config file",
                format!("{} is invalid YAML: {e}", path.display()),
                "edit `.firetrail/config.yml` to restore valid YAML",
            )),
        },
        Err(e) => checks.push(CheckResult::fail(
            "config.read",
            "Config file",
            format!("could not read {}: {e}", path.display()),
            "verify filesystem permissions",
        )),
    }
}

fn check_identity(root: &Path, checks: &mut Vec<CheckResult>) {
    let resolver = DefaultResolver::new(root, false);
    match resolver.resolve_with_trace() {
        Ok(trace) => {
            if let Some(id) = trace.resolved_identity {
                checks.push(CheckResult::ok(
                    "identity.resolve",
                    "Identity",
                    format!("resolved to `{}`", id.as_str()),
                ));
            } else {
                checks.push(CheckResult::warn(
                    "identity.resolve",
                    "Identity",
                    "no identity resolvable from any source",
                    "set `FIRETRAIL_AUTHOR` or `git config user.email`",
                ));
            }
        }
        Err(e) => checks.push(CheckResult::warn(
            "identity.resolve",
            "Identity",
            format!("resolution failed: {e}"),
            "set `FIRETRAIL_AUTHOR` or `git config user.email`",
        )),
    }
}

fn check_git(root: &Path, checks: &mut Vec<CheckResult>) {
    match Repo::open(root) {
        Ok(repo) => {
            let branch = repo.current_branch().ok().flatten();
            let detached = repo.is_detached().unwrap_or(false);
            let clean = repo.is_clean().unwrap_or(false);
            let summary = format!(
                "branch={} clean={clean} detached={detached}",
                branch.as_deref().unwrap_or("<detached>")
            );
            if detached {
                checks.push(CheckResult::warn(
                    "git.head",
                    "Git status",
                    summary,
                    "check out a branch before mutating the work graph",
                ));
            } else {
                checks.push(CheckResult::ok("git.head", "Git status", summary));
            }
        }
        Err(e) => checks.push(CheckResult::fail(
            "git.open",
            "Git status",
            format!("could not open repo: {e}"),
            "ensure the working directory is a git repository",
        )),
    }
}

fn check_hooks(root: &Path, checks: &mut Vec<CheckResult>) {
    let Ok(repo) = Repo::open(root) else {
        // The git.open check above will already have reported this.
        return;
    };
    let wanted = [
        HookName::PreCommit,
        HookName::PostCheckout,
        HookName::PostMerge,
    ];
    let missing: Vec<&'static str> = wanted
        .iter()
        .filter(|h| !repo.hook_installed(**h))
        .map(|h| h.filename())
        .collect();
    if missing.is_empty() {
        checks.push(CheckResult::ok(
            "git.hooks",
            "Git hooks",
            "all firetrail hooks installed",
        ));
    } else {
        checks.push(CheckResult::warn(
            "git.hooks",
            "Git hooks",
            format!("missing: {}", missing.join(", ")),
            "run `firetrail init` to (re)install hooks",
        ));
    }
}

fn check_index(ws: &workspace::Workspace, fix: bool, checks: &mut Vec<CheckResult>) {
    let db = ws.index_db_path();
    if !db.exists() {
        if fix {
            match EmbeddedStorage::open(&ws.root) {
                Ok(storage) => match Index::open(&ws.root) {
                    Ok(mut idx) => {
                        match idx.rebuild_from(&index_adapter::IndexStorage::new(&storage)) {
                            Ok(_) => checks.push(CheckResult {
                                id: "index.integrity".into(),
                                title: "Index integrity".into(),
                                status: CheckStatus::Ok,
                                message: "index.db missing — rebuilt from storage".into(),
                                suggestion: Some("firetrail index rebuild".into()),
                                fix_applied: true,
                            }),
                            Err(e) => checks.push(CheckResult::fail(
                                "index.integrity",
                                "Index integrity",
                                format!("index.db missing and rebuild failed: {e}"),
                                "firetrail index rebuild",
                            )),
                        }
                    }
                    Err(e) => checks.push(CheckResult::fail(
                        "index.integrity",
                        "Index integrity",
                        format!("could not open new index: {e}"),
                        "firetrail index rebuild",
                    )),
                },
                Err(e) => checks.push(CheckResult::fail(
                    "index.integrity",
                    "Index integrity",
                    format!("index.db missing and storage unavailable: {e}"),
                    "firetrail init && firetrail index rebuild",
                )),
            }
        } else {
            checks.push(CheckResult::fail(
                "index.integrity",
                "Index integrity",
                "`.firetrail/index.db` is missing",
                "firetrail index rebuild",
            ));
        }
        return;
    }

    match Index::open(&ws.root) {
        Ok(idx) => {
            let v = idx.schema_version();
            if v == 0 {
                checks.push(CheckResult::fail(
                    "index.schema",
                    "Index schema",
                    "schema version is 0 (unmigrated)",
                    "firetrail index rebuild",
                ));
            } else {
                checks.push(CheckResult::ok(
                    "index.schema",
                    "Index schema",
                    format!("schema version {v}"),
                ));
            }
            checks.push(CheckResult::ok(
                "index.integrity",
                "Index integrity",
                "index.db opens and migrates cleanly",
            ));
        }
        Err(e) => checks.push(CheckResult::fail(
            "index.integrity",
            "Index integrity",
            format!("could not open index.db: {e}"),
            "firetrail index rebuild",
        )),
    }
}

fn check_storage_parity(ws: &workspace::Workspace, checks: &mut Vec<CheckResult>) {
    use ft_storage::{Storage, StorageFilter};

    let storage = match EmbeddedStorage::open(&ws.root) {
        Ok(s) => s,
        Err(e) => {
            checks.push(CheckResult::fail(
                "storage.open",
                "Storage",
                format!("could not open storage: {e}"),
                "firetrail init",
            ));
            return;
        }
    };
    let on_disk = match storage.list(&StorageFilter::default()) {
        Ok(v) => v.len(),
        Err(e) => {
            checks.push(CheckResult::warn(
                "storage.list",
                "Storage",
                format!("could not list records: {e}"),
                "inspect `.firetrail/records/`",
            ));
            return;
        }
    };

    // No index parity check until M1 work-graph commands populate it; this
    // check intentionally reports the count and notes the index parity check
    // becomes meaningful once work-graph CRUD ships.
    checks.push(CheckResult::ok(
        "storage.count",
        "Storage records",
        format!("{on_disk} records on disk"),
    ));
}

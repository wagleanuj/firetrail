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

    // M3: embedding-related checks.
    check_cache_integrity(&ws, &mut checks);
    check_daemon_liveness(&ws, &mut checks);
    check_search_schema(&ws, &mut checks);

    // M5: registry / scope / claim-consistency / external-sync checks.
    check_identity_registry(&ws, &mut checks);
    check_scope_registry(&ws, &mut checks);
    check_claim_consistency(&ws, &mut checks);
    check_external_sync(&ws, &mut checks);

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
                    Ok(mut idx) => match idx.rebuild_from(&storage) {
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
                    },
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

fn check_cache_integrity(ws: &workspace::Workspace, checks: &mut Vec<CheckResult>) {
    // Cache lives **machine-local** under `~/.cache/firetrail/<repo-hash>/`
    // (or `$FIRETRAIL_CACHE_HOME/firetrail/<repo-hash>/`) so multiple
    // worktrees of the same repo share it (ADR-0007). We probe that path,
    // not the workspace-local `.firetrail/cache/`.
    let cache_db = match ft_embed::repo_cache_dir(&ws.root) {
        Ok(dir) => dir.join("embeddings.db"),
        Err(e) => {
            checks.push(CheckResult::warn(
                "embed.cache",
                "Embedding cache",
                format!("could not resolve cache directory: {e}"),
                "set $FIRETRAIL_CACHE_HOME or ensure $HOME is set",
            ));
            return;
        }
    };
    let hint_remove = format!(
        "remove `{}` to force a rebuild",
        cache_db.display()
    );
    if !cache_db.exists() {
        checks.push(CheckResult::ok(
            "embed.cache",
            "Embedding cache",
            format!("no cache yet at {} (will be created on first use)", cache_db.display()),
        ));
        return;
    }
    match ft_embed::EmbeddingCache::open_under(&ws.root) {
        Ok(cache) => match cache.verify_integrity() {
            Ok(report) => {
                if report.bad.is_empty() {
                    checks.push(CheckResult::ok(
                        "embed.cache",
                        "Embedding cache",
                        format!("{} rows clean ({})", report.scanned, cache_db.display()),
                    ));
                } else {
                    checks.push(CheckResult::fail(
                        "embed.cache",
                        "Embedding cache",
                        format!(
                            "{} rows scanned, {} with bad integrity checksum",
                            report.scanned,
                            report.bad.len()
                        ),
                        &hint_remove,
                    ));
                }
            }
            Err(e) => checks.push(CheckResult::warn(
                "embed.cache",
                "Embedding cache",
                format!("verify_integrity failed: {e}"),
                &hint_remove,
            )),
        },
        Err(e) => checks.push(CheckResult::warn(
            "embed.cache",
            "Embedding cache",
            format!("could not open cache: {e}"),
            "verify filesystem permissions on the machine-local cache directory",
        )),
    }
}

fn check_daemon_liveness(ws: &workspace::Workspace, checks: &mut Vec<CheckResult>) {
    let socket = ws.daemon_socket_path();
    let status = ft_embed::daemon::status(&socket);
    match status {
        ft_embed::DaemonStatus::Running => checks.push(CheckResult::ok(
            "embed.daemon",
            "Embedding daemon",
            format!("running at {}", socket.display()),
        )),
        ft_embed::DaemonStatus::Stopped => checks.push(CheckResult::ok(
            "embed.daemon",
            "Embedding daemon",
            "not running (search falls back to lexical-only mode)",
        )),
        ft_embed::DaemonStatus::Unreachable => checks.push(CheckResult::warn(
            "embed.daemon",
            "Embedding daemon",
            format!("socket exists but did not respond: {}", socket.display()),
            "run `firetrail daemon stop` then `firetrail daemon start`",
        )),
    }
}

fn check_identity_registry(ws: &workspace::Workspace, checks: &mut Vec<CheckResult>) {
    match ft_identity::load_registry(&ws.root) {
        Ok(reg) => checks.push(CheckResult::ok(
            "identity.registry",
            "Identity registry",
            format!("{} identities registered", reg.identities.len()),
        )),
        Err(e) => checks.push(CheckResult::fail(
            "identity.registry",
            "Identity registry",
            format!("failed to load `.firetrail/identities.yaml`: {e}"),
            "edit or remove `.firetrail/identities.yaml` to restore valid YAML",
        )),
    }
}

fn check_scope_registry(ws: &workspace::Workspace, checks: &mut Vec<CheckResult>) {
    match ft_scope::load(&ws.root) {
        Ok(reg) => checks.push(CheckResult::ok(
            "scope.registry",
            "Scope registry",
            format!("{} scopes configured", reg.scopes().len()),
        )),
        Err(e) => checks.push(CheckResult::fail(
            "scope.registry",
            "Scope registry",
            format!("failed to load `.firetrail/scopes.yaml`: {e}"),
            "edit `.firetrail/scopes.yaml` to restore valid YAML / globs",
        )),
    }
}

/// Walk every record once and flag any claim whose `claim_expires_at` is in
/// the past. Expired live claims should be released (or taken over).
fn check_claim_consistency(ws: &workspace::Workspace, checks: &mut Vec<CheckResult>) {
    use chrono::Utc;
    use ft_core::RecordBody;
    use ft_storage::{Storage, StorageFilter};

    let Ok(storage) = ft_storage::EmbeddedStorage::open(&ws.root) else {
        return;
    };
    let Ok(ids) = storage.list(&StorageFilter::default()) else {
        return;
    };
    let now = Utc::now();
    let mut expired: Vec<String> = Vec::new();
    for id in &ids {
        let Ok(rec) = storage.read(id) else { continue };
        let claim = match &rec.body {
            RecordBody::Task(t) => t.claim.as_ref(),
            RecordBody::Subtask(s) => s.claim.as_ref(),
            RecordBody::Bug(b) => b.claim.as_ref(),
            _ => None,
        };
        if let Some(c) = claim {
            if now >= c.claim_expires_at {
                expired.push(id.as_str().to_string());
            }
        }
    }
    if expired.is_empty() {
        checks.push(CheckResult::ok(
            "claim.consistency",
            "Claim consistency",
            "no expired-but-active claims",
        ));
    } else {
        checks.push(CheckResult::warn(
            "claim.consistency",
            "Claim consistency",
            format!(
                "{} expired claim(s) still attached: {:?}",
                expired.len(),
                expired
            ),
            "use `firetrail claim-takeover <id>` to release each",
        ));
    }
}

fn check_external_sync(ws: &workspace::Workspace, checks: &mut Vec<CheckResult>) {
    match ft_storage::StorageMode::from_workspace(&ws.root) {
        Ok(ft_storage::StorageMode::Embedded { .. }) => {
            checks.push(CheckResult::ok(
                "storage.mode",
                "Storage mode",
                "embedded",
            ));
        }
        Ok(ft_storage::StorageMode::External { config, .. }) => {
            match ft_storage::ExternalStorage::open(&ws.root, &config) {
                Ok(ext) => match ft_storage::sync_status(&ext) {
                    Ok(st) => {
                        let msg = format!(
                            "external ahead={} behind={} dirty={}",
                            st.ahead, st.behind, st.dirty
                        );
                        if st.behind > 0 || st.dirty {
                            checks.push(CheckResult::warn(
                                "storage.mode",
                                "Storage mode",
                                msg,
                                "run `firetrail sync` to reconcile",
                            ));
                        } else {
                            checks.push(CheckResult::ok("storage.mode", "Storage mode", msg));
                        }
                    }
                    Err(e) => checks.push(CheckResult::warn(
                        "storage.mode",
                        "Storage mode",
                        format!("external sync_status failed: {e}"),
                        "run `firetrail sync` and inspect the clone at `.firetrail/cache/data-repo`",
                    )),
                },
                Err(e) => checks.push(CheckResult::fail(
                    "storage.mode",
                    "Storage mode",
                    format!("could not open external storage: {e}"),
                    "verify `storage.data_repo_url` in `.firetrail/config.yml` and run `firetrail sync`",
                )),
            }
        }
        Err(_) => {
            // The config.* check already flagged this; nothing more to add.
        }
    }
}

fn check_search_schema(ws: &workspace::Workspace, checks: &mut Vec<CheckResult>) {
    let db = ws.index_db_path();
    if !db.exists() {
        // index.integrity check above will already have flagged this.
        return;
    }
    match ft_search::SearchEngine::open(&db) {
        Ok(engine) => match engine.ensure_schema() {
            Ok(()) => checks.push(CheckResult::ok(
                "search.schema",
                "Search schema",
                "FTS5 schema present and current",
            )),
            Err(e) => checks.push(CheckResult::fail(
                "search.schema",
                "Search schema",
                format!("ensure_schema failed: {e}"),
                "firetrail index rebuild",
            )),
        },
        Err(e) => checks.push(CheckResult::fail(
            "search.schema",
            "Search schema",
            format!("could not open search engine: {e}"),
            "firetrail index rebuild",
        )),
    }
}

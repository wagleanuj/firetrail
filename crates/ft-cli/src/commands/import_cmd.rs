//! `firetrail import …` — M6 import surface.
//!
//! Wraps [`ft_import::import_dir`] with workspace plumbing and a JSON-shaped
//! [`ImportOutcome`] for downstream consumers. Quarantined records are written
//! to storage when `--apply` is supplied; otherwise the command runs as a
//! parse-only dry run.
//!
//! Firetrail itself does not talk to Jira, Confluence, or other external
//! issue trackers. The calling agent (typically an AI agent with its own
//! MCP servers for those systems) fetches the upstream content, writes it
//! as markdown to a directory, and invokes `firetrail import …`.

use std::path::Path;

use ft_import::{ImportKind, ImportOptions, ImportReport, import_dir};
use serde::Serialize;

use crate::cli::{GlobalOpts, ImportDirArgs, ImportRefreshArgs};
use crate::commands::CommandOutcome;
use crate::context::WorkCtx;
use crate::error::CliError;

const CMD_INCIDENTS: &str = "import incidents";
const CMD_ADRS: &str = "import adrs";
const CMD_RUNBOOKS: &str = "import runbooks";
const CMD_REFRESH: &str = "import refresh";

/// `firetrail import incidents <dir>`
pub fn incidents(args: &ImportDirArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    import_one(CMD_INCIDENTS, ImportKind::Incidents, args, global)
}

/// `firetrail import adrs <dir>`
pub fn adrs(args: &ImportDirArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    import_one(CMD_ADRS, ImportKind::Adrs, args, global)
}

/// `firetrail import runbooks <dir>`
pub fn runbooks(args: &ImportDirArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    import_one(CMD_RUNBOOKS, ImportKind::Runbooks, args, global)
}

fn import_one(
    command: &'static str,
    kind: ImportKind,
    args: &ImportDirArgs,
    global: &GlobalOpts,
) -> Result<CommandOutcome, CliError> {
    let mut ctx = WorkCtx::open(command, global.workspace.as_deref())?;
    let actor = ctx.actor()?;

    let dir: &Path = args.dir.as_ref();
    let mut opts = ImportOptions::new(actor);
    // `--apply` flips off the default dry-run mode; otherwise we stay dry.
    if args.apply {
        opts.dry_run = false;
        opts.apply = true;
    } else {
        opts.dry_run = true;
        opts.apply = false;
    }

    let report = import_dir(&ctx.storage, dir, kind, &opts)
        .map_err(|e| CliError::user(command, format!("import: {e}")))?;

    // When we wrote records, refresh the SQL + search indexes so the imports
    // are queryable immediately. The search FTS row carries no quarantine
    // marker — that filter lives at the CLI layer (see `search.rs`).
    if opts.apply && report.written > 0 {
        refresh_indexes(command, &mut ctx)?;
    }

    Ok(CommandOutcome::Import(ImportOutcome::from_report(
        command, kind, &report, &opts,
    )))
}

/// `firetrail import refresh` — no-op for `LocalMarkdown` sources.
pub fn refresh(args: &ImportRefreshArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let _ = WorkCtx::open(CMD_REFRESH, global.workspace.as_deref())?;
    Ok(CommandOutcome::Import(ImportOutcome {
        command: CMD_REFRESH,
        kind: "refresh",
        dir: None,
        target: args.id.clone(),
        dry_run: true,
        apply: false,
        files_seen: 0,
        parsed: 0,
        written: 0,
        failures: Vec::new(),
        records: Vec::new(),
        note: Some(
            "import refresh is a no-op for LocalMarkdown sources; re-run `firetrail import <kind> <dir>` to re-ingest."
                .to_string(),
        ),
        warnings: Vec::new(),
    }))
}

fn refresh_indexes(command: &'static str, ctx: &mut WorkCtx) -> Result<(), CliError> {
    use ft_search::SearchEngine;
    use ft_storage::{Storage as _, StorageFilter};

    // Refresh the SQL index for every record on disk.
    let ids = ctx
        .storage
        .list(&StorageFilter::default())
        .map_err(|e| CliError::internal(command, format!("list storage: {e}")))?;
    let paths: Vec<_> = ids.iter().map(|id| ctx.storage.path_for(id)).collect();
    ctx.index
        .refresh(&ctx.storage, &paths, &[])
        .map_err(|e| CliError::internal(command, format!("refresh index: {e}")))?;

    // Upsert each record into the search FTS table so `firetrail search`
    // (with `--include-quarantine`) can find them immediately.
    let engine = SearchEngine::open(ctx.index.db_path())
        .map_err(|e| CliError::internal(command, format!("open search: {e}")))?;
    engine
        .ensure_schema()
        .map_err(|e| CliError::internal(command, format!("ensure search schema: {e}")))?;
    for id in &ids {
        let rec = ctx
            .storage
            .read(id)
            .map_err(|e| CliError::internal(command, format!("read {id}: {e}")))?;
        engine
            .upsert_lexical(&rec)
            .map_err(|e| CliError::internal(command, format!("upsert search: {e}")))?;
    }
    Ok(())
}

/// JSON / markdown view for `firetrail import …`.
#[derive(Debug, Clone, Serialize)]
pub struct ImportOutcome {
    /// Stable command name.
    #[serde(skip)]
    pub command: &'static str,
    /// Kind label (incidents/adrs/runbooks/refresh).
    pub kind: &'static str,
    /// Directory the importer walked (when applicable).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dir: Option<String>,
    /// Target identifier for single-record imports / refresh.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// Dry-run mode flag.
    pub dry_run: bool,
    /// Apply-mode flag.
    pub apply: bool,
    /// Total `*.md` files visited.
    pub files_seen: usize,
    /// Files that parsed successfully.
    pub parsed: usize,
    /// Records written to storage. Always 0 in dry-run mode.
    pub written: usize,
    /// `(path, reason)` pairs for failed files.
    pub failures: Vec<ImportFailureView>,
    /// IDs of records produced (or that would have been produced in dry-run).
    pub records: Vec<String>,
    /// Optional human-readable note (e.g. for the refresh stub).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Non-fatal warnings.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<String>,
}

/// Serialised representation of one failed import.
#[derive(Debug, Clone, Serialize)]
pub struct ImportFailureView {
    /// Path of the offending file.
    pub path: String,
    /// Reason it failed.
    pub reason: String,
}

impl ImportOutcome {
    fn from_report(
        command: &'static str,
        kind: ImportKind,
        report: &ImportReport,
        opts: &ImportOptions,
    ) -> Self {
        Self {
            command,
            kind: kind_label(kind),
            dir: None,
            target: None,
            dry_run: opts.dry_run,
            apply: opts.apply,
            files_seen: report.files_seen,
            parsed: report.parsed,
            written: report.written,
            failures: report
                .failures
                .iter()
                .map(|(p, r)| ImportFailureView {
                    path: p.display().to_string(),
                    reason: r.clone(),
                })
                .collect(),
            records: report
                .records
                .iter()
                .map(|id| id.as_str().to_string())
                .collect(),
            note: None,
            warnings: Vec::new(),
        }
    }

    /// Markdown rendering.
    pub fn markdown(&self) -> String {
        use std::fmt::Write as _;
        let mode = if self.dry_run {
            "dry-run"
        } else if self.apply {
            "apply"
        } else {
            "noop"
        };
        let mut s = format!(
            "**{}** ({}) — parsed:{} written:{} failed:{}\n",
            self.command,
            mode,
            self.parsed,
            self.written,
            self.failures.len(),
        );
        if let Some(note) = &self.note {
            let _ = writeln!(s, "_note: {note}_");
        }
        s
    }

    /// Quiet one-liner.
    pub fn quiet_line(&self) -> String {
        format!(
            "{}: parsed={} written={} failed={}",
            self.command,
            self.parsed,
            self.written,
            self.failures.len()
        )
    }
}

fn kind_label(kind: ImportKind) -> &'static str {
    match kind {
        ImportKind::Incidents => "incidents",
        ImportKind::Adrs => "adrs",
        ImportKind::Runbooks => "runbooks",
    }
}

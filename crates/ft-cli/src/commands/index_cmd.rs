//! `firetrail index rebuild` / `firetrail index refresh`.
//!
//! Rebuild drops the relational and search indexes and reconstructs them
//! from storage in one pass. Refresh re-applies only paths that look out of
//! date relative to storage; for M3 we keep this conservative and rebuild
//! the FTS table fully (it is small and the cost is negligible at current
//! record counts).

use ft_index::Index;
use ft_search::SearchEngine;
use ft_storage::{EmbeddedStorage, Storage as _, StorageFilter};
use serde::Serialize;

use crate::cli::GlobalOpts;
use crate::commands::CommandOutcome;
use crate::error::CliError;
use crate::workspace;

const CMD_REBUILD: &str = "index rebuild";
const CMD_REFRESH: &str = "index refresh";

/// `firetrail index rebuild`
pub fn rebuild(global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let ws = workspace::require_initialised(CMD_REBUILD, global.workspace.as_deref())?;
    let storage =
        EmbeddedStorage::open(&ws.root).map_err(|e| CliError::internal(CMD_REBUILD, e))?;
    let mut index = Index::open(&ws.root).map_err(|e| CliError::internal(CMD_REBUILD, e))?;
    let report = index
        .rebuild_from(&storage)
        .map_err(|e| CliError::internal(CMD_REBUILD, format!("rebuild SQL index: {e}")))?;

    // Search FTS table: rebuild by deleting every existing row and re-
    // upserting from storage. `SearchEngine` is a separate connection to the
    // same SQLite file; ensure schema first so a fresh workspace lights up.
    let engine = SearchEngine::open(index.db_path())
        .map_err(|e| CliError::internal(CMD_REBUILD, format!("open search: {e}")))?;
    engine
        .ensure_schema()
        .map_err(|e| CliError::internal(CMD_REBUILD, format!("ensure search schema: {e}")))?;
    let mut search_rows = 0usize;
    for row in storage.iter(&StorageFilter::default()) {
        let rec = row.map_err(|e| CliError::internal(CMD_REBUILD, format!("read record: {e}")))?;
        engine
            .upsert_lexical(&rec)
            .map_err(|e| CliError::internal(CMD_REBUILD, format!("upsert search: {e}")))?;
        search_rows += 1;
    }

    Ok(CommandOutcome::IndexAction(IndexActionOutcome {
        command: CMD_REBUILD,
        action: "rebuild",
        records_indexed: report.records_indexed,
        records_changed: report.records_indexed,
        search_rows_upserted: search_rows,
        warnings: Vec::new(),
    }))
}

/// `firetrail index refresh`
pub fn refresh(global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let ws = workspace::require_initialised(CMD_REFRESH, global.workspace.as_deref())?;
    let storage =
        EmbeddedStorage::open(&ws.root).map_err(|e| CliError::internal(CMD_REFRESH, e))?;
    let mut index = Index::open(&ws.root).map_err(|e| CliError::internal(CMD_REFRESH, e))?;

    // For M3, a "refresh" is a full re-scan with `refresh()` semantics —
    // collect every record path under storage and hand it to the index.
    // Records that haven't changed are no-ops at the index layer.
    let ids = storage
        .list(&StorageFilter::default())
        .map_err(|e| CliError::internal(CMD_REFRESH, e))?;
    let mut paths = Vec::with_capacity(ids.len());
    for id in &ids {
        paths.push(storage.path_for(id));
    }
    let report = index
        .refresh(&storage, &paths, &[])
        .map_err(|e| CliError::internal(CMD_REFRESH, format!("refresh SQL index: {e}")))?;

    let engine = SearchEngine::open(index.db_path())
        .map_err(|e| CliError::internal(CMD_REFRESH, format!("open search: {e}")))?;
    engine
        .ensure_schema()
        .map_err(|e| CliError::internal(CMD_REFRESH, format!("ensure search schema: {e}")))?;
    let mut search_rows = 0usize;
    for id in &ids {
        let rec = storage
            .read(id)
            .map_err(|e| CliError::internal(CMD_REFRESH, format!("read {id}: {e}")))?;
        engine
            .upsert_lexical(&rec)
            .map_err(|e| CliError::internal(CMD_REFRESH, format!("upsert search: {e}")))?;
        search_rows += 1;
    }

    Ok(CommandOutcome::IndexAction(IndexActionOutcome {
        command: CMD_REFRESH,
        action: "refresh",
        records_indexed: report.records_added + report.records_updated,
        records_changed: report.records_added + report.records_updated + report.records_removed,
        search_rows_upserted: search_rows,
        warnings: Vec::new(),
    }))
}

/// JSON view for `firetrail index {rebuild,refresh}`.
#[derive(Debug, Clone, Serialize)]
pub struct IndexActionOutcome {
    /// Stable command name.
    #[serde(skip)]
    pub command: &'static str,
    /// Action label.
    pub action: &'static str,
    /// Records written to the SQL index.
    pub records_indexed: u64,
    /// Records whose state changed (added + updated + removed for refresh).
    pub records_changed: u64,
    /// Rows upserted into the search FTS table.
    pub search_rows_upserted: usize,
    /// Non-fatal warnings.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<String>,
}

impl IndexActionOutcome {
    /// Markdown rendering.
    pub fn markdown(&self) -> String {
        format!(
            "**index {}** indexed={} changed={} search_rows={}\n",
            self.action, self.records_indexed, self.records_changed, self.search_rows_upserted
        )
    }
    /// Quiet line.
    pub fn quiet_line(&self) -> String {
        format!(
            "index {}: indexed={} changed={} search={}",
            self.action, self.records_indexed, self.records_changed, self.search_rows_upserted
        )
    }
}

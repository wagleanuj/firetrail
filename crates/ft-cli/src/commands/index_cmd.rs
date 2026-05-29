//! `firetrail index rebuild` / `firetrail index refresh`.
//!
//! Rebuild drops the relational and search indexes and reconstructs them
//! from storage in one pass. Refresh re-applies only paths that look out of
//! date relative to storage; for M3 we keep this conservative and rebuild
//! the FTS table fully (it is small and the cost is negligible at current
//! record counts).

use chrono::Utc;
use ft_index::Index;
use ft_scope::ScopeRegistry;
use ft_search::{IndexDoc, SearchEngine};
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
    let mut warnings: Vec<String> = Vec::new();
    let mut records: Vec<ft_core::Record> = Vec::new();
    for row in storage.iter(&StorageFilter::default()) {
        let rec = row.map_err(|e| CliError::internal(CMD_REBUILD, format!("read record: {e}")))?;
        engine
            .upsert_lexical(&rec)
            .map_err(|e| CliError::internal(CMD_REBUILD, format!("upsert search: {e}")))?;
        search_rows += 1;
        records.push(rec);
    }
    let synthetic = index_synthetic_docs(CMD_REBUILD, &ws, &engine, &records, &mut warnings)?;
    search_rows += synthetic;

    Ok(CommandOutcome::IndexAction(IndexActionOutcome {
        command: CMD_REBUILD,
        action: "rebuild",
        records_indexed: report.records_indexed,
        records_changed: report.records_indexed,
        search_rows_upserted: search_rows,
        warnings,
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
    let mut warnings: Vec<String> = Vec::new();
    let mut records: Vec<ft_core::Record> = Vec::new();
    for id in &ids {
        let rec = storage
            .read(id)
            .map_err(|e| CliError::internal(CMD_REFRESH, format!("read {id}: {e}")))?;
        engine
            .upsert_lexical(&rec)
            .map_err(|e| CliError::internal(CMD_REFRESH, format!("upsert search: {e}")))?;
        search_rows += 1;
        records.push(rec);
    }
    let synthetic = index_synthetic_docs(CMD_REFRESH, &ws, &engine, &records, &mut warnings)?;
    search_rows += synthetic;

    Ok(CommandOutcome::IndexAction(IndexActionOutcome {
        command: CMD_REFRESH,
        action: "refresh",
        records_indexed: report.records_added + report.records_updated,
        records_changed: report.records_added + report.records_updated + report.records_removed,
        search_rows_upserted: search_rows,
        warnings,
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

/// Index scopes, identities, and per-entry audit history as synthetic
/// documents. `records` is the set of records already read in this pass (audit
/// entries are extracted from them). Returns the number of synthetic docs
/// upserted. Embedding is dispatched best-effort; failures degrade to
/// lexical-only and are surfaced as warnings.
fn index_synthetic_docs(
    cmd: &'static str,
    ws: &crate::workspace::Workspace,
    engine: &ft_search::SearchEngine,
    records: &[ft_core::Record],
    warnings: &mut Vec<String>,
) -> Result<usize, CliError> {
    let now = Utc::now();
    let mut docs: Vec<IndexDoc> = Vec::new();

    match ScopeRegistry::load(&ws.root) {
        Ok(reg) => {
            for scope in reg.scopes() {
                docs.push(ft_search::scope_doc(scope, now));
            }
        }
        Err(e) => warnings.push(format!("scope index skipped: {e}")),
    }

    match ft_identity::load_registry(&ws.root) {
        Ok(reg) => {
            for ident in &reg.identities {
                docs.push(ft_search::identity_doc(ident, now));
            }
        }
        Err(e) => warnings.push(format!("identity index skipped: {e}")),
    }

    for rec in records {
        let trust = audit_record_trust(rec);
        docs.extend(ft_search::audit_docs(rec, trust));
    }

    for doc in &docs {
        engine
            .upsert_document(doc)
            .map_err(|e| CliError::internal(cmd, format!("upsert synthetic doc: {e}")))?;
    }

    dispatch_synthetic_embeddings(ws, &docs, warnings);

    Ok(docs.len())
}

/// Trust an audit doc inherits — mirrors the engine's record-trust rule
/// (memory bodies carry trust; work kinds default to reviewed).
fn audit_record_trust(rec: &ft_core::Record) -> ft_core::TrustState {
    use ft_core::{RecordBody, TrustState};
    match &rec.body {
        RecordBody::Incident(b) => b.trust,
        RecordBody::Finding(b) => b.trust,
        RecordBody::Runbook(b) => b.trust,
        RecordBody::Decision(b) => b.trust,
        RecordBody::Gotcha(b) => b.trust,
        RecordBody::Memory(b) => b.trust,
        RecordBody::Doc(b) => b.trust,
        RecordBody::Epic(_) | RecordBody::Task(_) | RecordBody::Subtask(_) | RecordBody::Bug(_) => {
            TrustState::Reviewed
        }
    }
}

/// Send `IndexRecord` requests for synthetic docs so their vectors land.
///
/// Best effort, and deliberately **non-spawning**: we only dispatch when a
/// daemon is *already* running. `index rebuild` must not start a background
/// embedder, because that daemon would open the same `SQLite` database
/// concurrently with the rebuild's own connection (corrupting it) and outlive
/// the command (breaking subsequent index opens). Guaranteed embedding of
/// freshly-changed config/audit docs is handled incrementally on write
/// (firetrail-8z0m.5); here we opportunistically embed when a daemon already
/// exists (e.g. one auto-spawned by an earlier `search`), else stay
/// lexical-only.
fn dispatch_synthetic_embeddings(
    ws: &crate::workspace::Workspace,
    docs: &[IndexDoc],
    warnings: &mut Vec<String>,
) {
    if docs.is_empty() {
        return;
    }
    let socket = match ws.daemon_socket_path() {
        Ok(s) => s,
        Err(e) => {
            warnings.push(format!("synthetic-doc embedding skipped: {e}"));
            return;
        }
    };
    if ft_embed::daemon::status(&socket) != ft_embed::daemon::DaemonStatus::Running {
        warnings.push(
            "synthetic-doc embedding skipped: no embed daemon running (docs indexed \
             lexically; start the daemon or re-save to embed)"
                .to_string(),
        );
        return;
    }
    for doc in docs {
        let id = doc.id.as_storage_str();
        if let Err(e) = ft_embed::daemon::send_index_record(&socket, &id, &doc.embed_text()) {
            warnings.push(format!("embed {id} failed: {e}"));
        }
    }
}

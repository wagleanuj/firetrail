//! Shared helpers for the work-graph commands.
//!
//! Every work-graph subcommand follows the same setup pattern:
//!
//! 1. Locate the workspace.
//! 2. Open [`EmbeddedStorage`].
//! 3. Resolve an [`Identity`] for the actor.
//! 4. (Optionally) open the [`Index`] and refresh it after writes.
//!
//! This module centralises that plumbing so each command file stays focused on
//! its domain logic.
//!
//! ### Prefix resolution (ADR-0015)
//!
//! The CLI accepts either the full 69-char `<KIND>-<hex>` form or an
//! unambiguous prefix. [`resolve_record_id`] performs a simple "scan storage,
//! find unique candidate" pass — fine for M1 record counts; a tighter
//! index-backed resolver is filed as `firetrail-exs`.
//!
//! ### Interim relation store
//!
//! `link` / `dep` writes go to `.firetrail/relations.jsonl` (append-only,
//! newline-delimited JSON [`Relation`] records). `show` / `graph` re-read this
//! file to surface external relations alongside the structural ones the index
//! derives from `parent_epic` / `parent_task` / `child_ids`. The canonical
//! relation store is `firetrail-tq7`; until then, this file is the truth.

use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use ft_core::{Identity, Record, RecordId, Relation, ResolveError, resolve_prefix, state_hash};
use ft_history::{HistoryDraft, HistoryEntryKind, append_history};
use ft_identity::{DefaultResolver, IdentityResolver, load_registry};
use ft_index::Index;
use ft_search::SearchEngine;
use ft_storage::{EmbeddedStorage, ExternalStorage, Storage as _, StorageFilter, StorageMode};

use crate::error::CliError;
use crate::workspace::{self, Workspace};

/// Bundle of resources every work-graph command needs.
pub struct WorkCtx {
    /// Resolved workspace.
    pub ws: Workspace,
    /// On-disk record store. Rooted on the workspace in embedded mode, or on
    /// the data-repo clone path in external mode — either way every consumer
    /// reads/writes via the same `EmbeddedStorage` API.
    pub storage: EmbeddedStorage,
    /// External storage handle (Some when storage mode is external). The
    /// auto-commit semantics of `ExternalStorage::write` are layered on top
    /// of the embedded write in [`WorkCtx::save_record`].
    pub external: Option<ExternalStorage>,
    /// Index handle.
    pub index: Index,
    /// Non-fatal warnings to surface in the JSON envelope.
    pub warnings: Vec<String>,
    /// Identity to stamp on writes (lazily computed on first call).
    actor: Option<Identity>,
    /// Command name for error framing.
    command: String,
    /// Lazy search engine — opened on first request and reused. Each opening
    /// runs `ensure_schema` exactly once per invocation.
    search: Option<SearchEngine>,
}

impl WorkCtx {
    /// Open the workspace, storage, and index for `command`.
    ///
    /// If the index database is missing or fails to open (e.g. corrupt
    /// schema), the index is silently rebuilt from storage. A warning is
    /// emitted so the recovery is observable via the JSON envelope.
    pub fn open(command: &str, override_path: Option<&Path>) -> Result<Self, CliError> {
        let ws = workspace::require_initialised(command, override_path)?;

        let mut warnings = Vec::new();

        // Resolve storage mode from `.firetrail/config.yml`. External mode
        // opens (clones) the data repo and roots the EmbeddedStorage view on
        // the clone path; auto-pull is best-effort.
        let (storage, external) = match StorageMode::from_workspace(&ws.root) {
            Ok(StorageMode::Embedded { root }) => {
                let s = EmbeddedStorage::open(&root)
                    .map_err(|e| CliError::internal(command, format!("open storage: {e}")))?;
                (s, None)
            }
            Ok(StorageMode::External { config, .. }) => {
                let ext = ExternalStorage::open(&ws.root, &config).map_err(|e| {
                    CliError::internal(command, format!("open external storage: {e}"))
                })?;
                if let Err(e) = ext.pull() {
                    warnings.push(format!("external storage auto-pull failed: {e}"));
                }
                let s = EmbeddedStorage::open(ext.clone_path())
                    .map_err(|e| CliError::internal(command, format!("open clone storage: {e}")))?;
                (s, Some(ext))
            }
            Err(e) => {
                return Err(CliError::internal(
                    command,
                    format!("read storage config: {e}"),
                ));
            }
        };
        let db_path = ws.index_db_path();
        let needs_rebuild = !db_path.exists();
        let mut index = match Index::open(&ws.root) {
            Ok(idx) => idx,
            Err(e) => {
                // Corrupt schema or unmigratable file: nuke and reopen.
                let _ = std::fs::remove_file(&db_path);
                warnings.push(format!(
                    "index.db could not be opened ({e}); rebuilding from storage"
                ));
                Index::open(&ws.root)
                    .map_err(|e| CliError::internal(command, format!("reopen index: {e}")))?
            }
        };
        // Empty schema_version => freshly created or migrations missing.
        let needs_rebuild = needs_rebuild || index.schema_version() == 0;
        if needs_rebuild {
            if !warnings
                .iter()
                .any(|w| w.contains("rebuilding from storage"))
            {
                warnings.push("index.db was missing; rebuilt from storage".to_string());
            }
            index
                .rebuild_from(&storage)
                .map_err(|e| CliError::internal(command, format!("rebuild index: {e}")))?;
            tracing::debug!("auto-rebuilt missing/corrupt index for `{command}`");
        }

        Ok(Self {
            ws,
            storage,
            external,
            index,
            warnings,
            actor: None,
            command: command.to_string(),
            search: None,
        })
    }

    /// Open (lazily) the [`SearchEngine`] backed by the same `index.db`.
    ///
    /// `ensure_schema` is idempotent and is run on the first open.
    pub fn search_engine(&mut self) -> Result<&SearchEngine, CliError> {
        if self.search.is_none() {
            let db = self.index.db_path().to_path_buf();
            let engine = SearchEngine::open(&db)
                .map_err(|e| CliError::internal(&self.command, format!("open search: {e}")))?;
            engine.ensure_schema().map_err(|e| {
                CliError::internal(&self.command, format!("ensure search schema: {e}"))
            })?;
            self.search = Some(engine);
        }
        Ok(self.search.as_ref().expect("just set"))
    }

    /// Resolve and cache the identity of the actor.
    ///
    /// When the workspace's `config.yml` has `identity.strict: true`, the
    /// resolved actor must match an entry in `.firetrail/identities.yaml`
    /// (by canonical id or any registered email alias). An unregistered
    /// actor is rejected with a [`CliError::UserError`] so unauthorised
    /// writes never reach storage (firetrail-8ql).
    pub fn actor(&mut self) -> Result<Identity, CliError> {
        if let Some(id) = &self.actor {
            return Ok(id.clone());
        }
        let resolver = DefaultResolver::new(&self.ws.root, false);
        let id = resolver.resolve().map_err(|e| CliError::UserError {
            command: self.command.clone(),
            message: format!("identity unresolvable: {e}"),
            details: serde_json::json!({ "hint": "set FIRETRAIL_AUTHOR or `git config user.email`" }),
        })?;
        if strict_identity_enabled(&self.ws.root) {
            let registry = load_registry(&self.ws.root).map_err(|e| {
                CliError::internal(&self.command, format!("load identity registry: {e}"))
            })?;
            if registry.resolve_canonical(id.as_str()).is_none() {
                return Err(CliError::user(
                    &self.command,
                    format!(
                        "identity `{}` is not registered; run `firetrail identity register` or set identity.strict: false in .firetrail/config.yml",
                        id.as_str()
                    ),
                ));
            }
        }
        self.actor = Some(id.clone());
        Ok(id)
    }

    /// Resolve an id string (full or prefix) against on-disk storage.
    pub fn resolve_id(&self, raw: &str) -> Result<RecordId, CliError> {
        resolve_record_id(&self.command, &self.storage, raw)
    }

    /// Persist `record` after appending a history entry built from `draft`.
    ///
    /// This is the canonical "memory create / trust transition / runbook step"
    /// write path: it appends the history entry (which updates `state_hash`
    /// and `prev_state_hash`), then routes through [`Self::save_record`] so the
    /// write benefits from the same external auto-commit, index refresh, and
    /// search FTS upsert as work-graph writes.
    pub fn save_record_with_history(
        &mut self,
        record: &mut Record,
        draft: HistoryDraft,
    ) -> Result<PathBuf, CliError> {
        // Enforce strict-identity even for explicit-kind writes (firetrail-8ql).
        let _ = self.actor()?;
        append_history(record, draft)
            .map_err(|e| CliError::internal(&self.command, format!("history append: {e}")))?;
        // `append_history` has already rebuilt state_hash and the chain; skip
        // the auto-append path in `save_record` so we don't double-stamp.
        self.persist_record(record)
    }

    /// Persist `record`, appending an `Update` (or genesis `Create`) history
    /// entry, recomputing its `state_hash`, and refreshing the index so the
    /// change is queryable immediately.
    ///
    /// Every write goes through this choke point so the per-record history
    /// chain (and `prev_state_hash`) is populated for create / update / claim
    /// / close / criteria / link / dep paths (firetrail-65q). Callers that
    /// need an explicit history kind / summary use
    /// [`Self::save_record_with_history`] instead.
    pub fn save_record(&mut self, record: &mut Record) -> Result<PathBuf, CliError> {
        // Ensure strict-identity is enforced on every write, even when the
        // command did not explicitly resolve an actor (firetrail-8ql).
        let actor = self.actor()?;

        // Auto-append a history entry so the chain is always populated.
        // Genesis writes (no prior history, no prev pointer) get a `Create`
        // entry; subsequent writes get an `Update` entry. Callers that need
        // a specific kind (TrustTransition, Close, Reopen, …) route through
        // [`Self::save_record_with_history`].
        let kind = if record.envelope.history.is_empty()
            && record.envelope.prev_state_hash.is_none()
        {
            HistoryEntryKind::Create
        } else {
            HistoryEntryKind::Update
        };
        let kind_tag = record.envelope.kind.prefix().to_ascii_lowercase();
        let summary = match kind {
            HistoryEntryKind::Create => format!("{kind_tag} created via `{}`", self.command),
            _ => format!("{kind_tag} updated via `{}`", self.command),
        };
        let draft = HistoryDraft {
            merged_via_pr: None,
            timestamp: record.envelope.updated_at,
            primary_actor: actor,
            contributors: Vec::new(),
            ops_summary: vec![summary],
            ops_count: 1,
            kind,
        };
        append_history(record, draft)
            .map_err(|e| CliError::internal(&self.command, format!("history append: {e}")))?;

        self.persist_record(record)
    }

    /// Write `record` to storage (and external storage when configured) and
    /// refresh the index / search engines. Does NOT touch `history[]` or
    /// `prev_state_hash` — callers must have already populated those.
    fn persist_record(&mut self, record: &mut Record) -> Result<PathBuf, CliError> {
        record.envelope.state_hash = String::new();
        let new_hash = state_hash(record)
            .map_err(|e| CliError::internal(&self.command, format!("hash: {e}")))?;
        record.envelope.state_hash = new_hash;

        // In external mode, route the write through ExternalStorage so the
        // record is auto-committed in the data-repo clone. Both views
        // (embedded view on the clone path, and the external handle) share
        // the same underlying files.
        let path = if let Some(ext) = &self.external {
            ext.write(record)
                .map_err(|e| CliError::internal(&self.command, format!("write (external): {e}")))?
        } else {
            self.storage
                .write(record)
                .map_err(|e| CliError::internal(&self.command, format!("write: {e}")))?
        };

        // The index may have changed shape (status, claim, AC, …); a targeted
        // refresh is cheap and avoids rebuilds.
        self.index
            .refresh(&self.storage, std::slice::from_ref(&path), &[])
            .map_err(|e| CliError::internal(&self.command, format!("refresh: {e}")))?;

        // M3: upsert into the search index alongside the SQL index so search
        // results stay current with every write. Vector indexing is opt-in
        // and depends on a running daemon — at lexical-only level we still
        // get a usable `firetrail search`.
        self.upsert_search_lexical(record);

        // firetrail-0nu: embed-on-write hand-off. When the daemon is running
        // we send a synchronous IndexRecord request so the embedding is
        // available for vector search; this is best-effort — any failure
        // (daemon down, embedder error) is logged as a warning and never
        // blocks the write.
        self.try_dispatch_index_record(record);

        Ok(path)
    }

    /// Best-effort embed-on-write hand-off (firetrail-0nu).
    fn try_dispatch_index_record(&mut self, record: &Record) {
        let socket = match self.ws.daemon_socket_path() {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(error = %e, command = %self.command, "resolve daemon socket path");
                self.warnings
                    .push(format!("embed-on-write skipped: {e}"));
                return;
            }
        };
        if ft_embed::daemon::status(&socket) != ft_embed::DaemonStatus::Running {
            return;
        }
        let text = ft_embed::record_text(record);
        if let Err(e) =
            ft_embed::daemon::send_index_record(&socket, record.envelope.id.as_str(), &text)
        {
            tracing::warn!(error = %e, command = %self.command, "embed-on-write dispatch failed");
            self.warnings
                .push(format!("embed-on-write skipped: {e}"));
        }
    }

    /// Best-effort lexical upsert into the search index. Failures are
    /// recorded as warnings rather than propagated so a search-layer hiccup
    /// never blocks a write.
    fn upsert_search_lexical(&mut self, record: &Record) {
        let cmd = self.command.clone();
        match self.search_engine() {
            Ok(engine) => {
                if let Err(e) = engine.upsert_lexical(record) {
                    tracing::warn!(error = %e, command = %cmd, "search upsert failed");
                    self.warnings
                        .push(format!("search index upsert skipped: {e}"));
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, command = %cmd, "search engine unavailable");
                self.warnings
                    .push(format!("search engine unavailable: {e}"));
            }
        }
    }

    /// Read a record by id from storage.
    pub fn read_record(&self, id: &RecordId) -> Result<Record, CliError> {
        self.storage.read(id).map_err(|e| match e {
            ft_storage::StorageError::NotFound(_) => CliError::NotFound {
                command: self.command.clone(),
                what: id.as_str().to_string(),
            },
            other => CliError::internal(&self.command, other),
        })
    }
}

/// Resolve `raw` as either a full record id or a unique prefix.
///
/// Delegates the prefix-matching logic to [`ft_core::resolve_prefix`] over the
/// full storage listing. `ResolveError` variants are mapped onto the
/// appropriate [`CliError`] surface: `Unknown` → `NotFound`, `Ambiguous` →
/// `UserError` with a structured match list, `Empty` → `UserError`.
pub fn resolve_record_id(
    command: &str,
    storage: &EmbeddedStorage,
    raw: &str,
) -> Result<RecordId, CliError> {
    // Fast path: a fully-canonical id skips the storage scan. This avoids
    // validating every record on disk (which `storage.list` does, surfacing
    // hash-mismatch errors from unrelated records) when the caller already
    // typed the precise id they want.
    if let Ok(id) = RecordId::from_string(raw.to_string()) {
        return if storage.read(&id).is_ok() {
            Ok(id)
        } else {
            Err(CliError::NotFound {
                command: command.to_string(),
                what: raw.to_string(),
            })
        };
    }

    let candidates = storage.list(&StorageFilter::default()).map_err(|e| {
        CliError::internal(command, format!("scanning storage for prefix match: {e}"))
    })?;
    match resolve_prefix(raw, &candidates) {
        Ok(id) => Ok(id),
        Err(ResolveError::Empty) => Err(CliError::UserError {
            command: command.to_string(),
            message: "empty record id".to_string(),
            details: serde_json::Value::Null,
        }),
        Err(ResolveError::EmptyHexPrefix(kind)) => Err(CliError::UserError {
            command: command.to_string(),
            message: format!("hex prefix is required after kind tag `{kind}`"),
            details: serde_json::json!({ "kind": kind }),
        }),
        Err(ResolveError::Unknown(_)) => Err(CliError::NotFound {
            command: command.to_string(),
            what: raw.to_string(),
        }),
        Err(ResolveError::Ambiguous { prefix, matches }) => {
            let preview: Vec<String> = matches
                .iter()
                .take(5)
                .map(|m| m.short(MIN_SHORT_HEX).to_string())
                .collect();
            Err(CliError::UserError {
                command: command.to_string(),
                message: format!(
                    "`{raw}` is ambiguous; matches {n} records (showing up to 5): {preview}",
                    n = matches.len(),
                    preview = preview.join(", "),
                ),
                details: serde_json::json!({
                    "ambiguous_prefix": prefix,
                    "matches": matches.iter().map(|m| m.as_str().to_string()).collect::<Vec<_>>(),
                }),
            })
        }
    }
}

/// Display length (in hex chars) for short-form ids in ambiguity messages.
const MIN_SHORT_HEX: usize = 8;

/// Path of the interim relation log.
#[must_use]
pub fn relations_log_path(ws: &Workspace) -> PathBuf {
    ws.firetrail_dir().join("relations.jsonl")
}

/// Append a relation to the interim log.
pub fn append_relation(ws: &Workspace, relation: &Relation) -> Result<(), CliError> {
    let path = relations_log_path(ws);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| CliError::internal("link", format!("ensure relations dir: {e}")))?;
    }
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| CliError::internal("link", format!("open relations log: {e}")))?;
    let line = serde_json::to_string(relation)
        .map_err(|e| CliError::internal("link", format!("encode relation: {e}")))?;
    writeln!(f, "{line}")
        .map_err(|e| CliError::internal("link", format!("write relations log: {e}")))?;
    Ok(())
}

/// Load every relation currently in the log.
pub fn load_relations(ws: &Workspace) -> Result<Vec<Relation>, CliError> {
    let path = relations_log_path(ws);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let f = std::fs::File::open(&path)
        .map_err(|e| CliError::internal("show", format!("open relations log: {e}")))?;
    let mut out = Vec::new();
    for (lineno, line) in BufReader::new(f).lines().enumerate() {
        let line =
            line.map_err(|e| CliError::internal("show", format!("read relations line: {e}")))?;
        if line.trim().is_empty() {
            continue;
        }
        let rel: Relation = serde_json::from_str(&line).map_err(|e| {
            CliError::internal("show", format!("parse relations line {}: {e}", lineno + 1))
        })?;
        out.push(rel);
    }
    Ok(out)
}

/// Overwrite the relation log with `relations`.
pub fn rewrite_relations(ws: &Workspace, relations: &[Relation]) -> Result<(), CliError> {
    let path = relations_log_path(ws);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| CliError::internal("dep", format!("ensure relations dir: {e}")))?;
    }
    let mut s = String::new();
    for r in relations {
        let line = serde_json::to_string(r)
            .map_err(|e| CliError::internal("dep", format!("encode relation: {e}")))?;
        s.push_str(&line);
        s.push('\n');
    }
    std::fs::write(&path, s)
        .map_err(|e| CliError::internal("dep", format!("rewrite relations log: {e}")))?;
    Ok(())
}

/// Whether `.firetrail/config.yml` opts the workspace into strict identity
/// enforcement (firetrail-8ql). A missing file or missing key both mean
/// "lenient" — preserves the M1 resolver behaviour for unregistered repos.
fn strict_identity_enabled(root: &Path) -> bool {
    #[derive(serde::Deserialize)]
    struct StrictFlag {
        strict: Option<bool>,
    }
    #[derive(serde::Deserialize)]
    struct ConfigShell {
        identity: Option<StrictFlag>,
    }
    let path = root.join(".firetrail").join("config.yml");
    let Ok(s) = std::fs::read_to_string(&path) else {
        return false;
    };
    let parsed: ConfigShell = match serde_yaml::from_str(&s) {
        Ok(v) => v,
        Err(_) => return false,
    };
    parsed
        .identity
        .and_then(|i| i.strict)
        .unwrap_or(false)
}

/// Parse a `key=value` label argument; returns the (key, value) pair or a user
/// error if the `=` separator is missing.
pub fn parse_label_pair(command: &str, raw: &str) -> Result<(String, String), CliError> {
    let (k, v) = raw.split_once('=').ok_or_else(|| CliError::UserError {
        command: command.to_string(),
        message: format!("label `{raw}` must be in `key=value` form"),
        details: serde_json::json!({ "label": raw }),
    })?;
    if k.trim().is_empty() {
        return Err(CliError::UserError {
            command: command.to_string(),
            message: format!("label key in `{raw}` is empty"),
            details: serde_json::Value::Null,
        });
    }
    Ok((k.trim().to_string(), v.trim().to_string()))
}

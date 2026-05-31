//! `Index` — open, migrate, rebuild, refresh, query.

use std::collections::{HashSet, VecDeque};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Instant;

use chrono::{DateTime, Utc};
use ft_core::{
    AcStatus, AcceptanceCriterion, Claim, Evidence, EvidenceKind, Identity, Origin, Priority,
    Record, RecordBody, RecordId, RecordKind, RelationKind, Status,
};
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};

use ft_storage::{Storage, StorageFilter};

use crate::error::IndexError;
use crate::schema;
use crate::types::{
    DepEdge, IndexedRecord, ListQuery, OrderBy, ReadyQuery, RebuildReport, RefreshReport,
    WalkDirection,
};

/// Threshold above which an "incremental" refresh promotes itself to a full
/// rebuild. Matches the default mentioned in `docs/components/ft-index.md`.
const REFRESH_FULL_REBUILD_THRESHOLD: usize = 500;

/// `SQLite`-backed read index over the JSON-in-Git record store.
///
/// The database lives at `<workspace>/.firetrail/index.db`. It is gitignored
/// and rebuildable from `Storage` at any time.
///
/// # Examples
///
/// ```
/// # use tempfile::tempdir;
/// # use ft_index::Index;
/// let dir = tempdir().unwrap();
/// std::fs::create_dir_all(dir.path().join(".firetrail")).unwrap();
/// let index = Index::open(dir.path()).unwrap();
/// assert_eq!(index.schema_version(), 1);
/// ```
pub struct Index {
    db_path: PathBuf,
    conn: Connection,
}

impl std::fmt::Debug for Index {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Index")
            .field("db_path", &self.db_path)
            .finish_non_exhaustive()
    }
}

impl Index {
    /// Open (or create) the index database under `<workspace_root>/.firetrail/`.
    ///
    /// Applies any pending migrations.
    pub fn open(workspace_root: impl AsRef<Path>) -> Result<Self, IndexError> {
        let workspace_root = workspace_root.as_ref();
        let dir = workspace_root.join(".firetrail");
        std::fs::create_dir_all(&dir)?;
        let db_path = dir.join("index.db");

        let mut conn = Connection::open(&db_path)?;
        schema::apply_pragmas(&conn)?;
        schema::apply_pending(&mut conn)?;

        Ok(Self { db_path, conn })
    }

    /// On-disk schema version.
    pub fn schema_version(&self) -> u32 {
        schema::read_version(&self.conn).unwrap_or(0)
    }

    /// Absolute path to the `SQLite` database file.
    #[must_use]
    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    /// Reapply pending migrations.
    ///
    /// Normally called automatically by [`Self::open`]. Exposed for callers
    /// that want to re-check after a binary upgrade without reopening.
    pub fn migrate(&mut self) -> Result<(), IndexError> {
        schema::apply_pending(&mut self.conn)
    }

    // ── Writes ──────────────────────────────────────────────────────────────

    /// Drop all data and rebuild from `storage`.
    ///
    /// Runs in a single transaction: either fully succeeds or leaves the
    /// previous index intact.
    pub fn rebuild_from(&mut self, storage: &dyn Storage) -> Result<RebuildReport, IndexError> {
        let start = Instant::now();
        let mut report = RebuildReport::default();

        // Collect first so we can iterate twice (records, then relations).
        // ft_storage::Storage::iter already yields every record without a
        // closed/archived filter; the index intentionally tracks every kind.
        let filter = StorageFilter::default();
        let mut all: Vec<(Record, PathBuf)> = Vec::new();
        for row in storage.iter(&filter) {
            let record = row?;
            let path = storage.path_for(&record.envelope.id);
            all.push((record, path));
        }

        let tx = self.conn.transaction()?;

        // Drop data tables but keep schema_meta.
        tx.execute_batch(
            "DELETE FROM evidence;
             DELETE FROM acceptance_criteria;
             DELETE FROM claims;
             DELETE FROM relations;
             DELETE FROM applies_to;
             DELETE FROM affected_scopes;
             DELETE FROM labels;
             DELETE FROM records;",
        )?;

        for (record, path) in &all {
            insert_record(&tx, record, path)?;
            report.records_indexed += 1;
        }

        // Second pass: derive and insert relation edges from inline fields.
        for (record, _) in &all {
            let edges = relations_for(record);
            for edge in edges {
                insert_relation(&tx, &edge)?;
                report.relations_indexed += 1;
            }
        }

        // Ingest non-structural edges (`blocked-by`, `related-to`, ...) via
        // the `Storage::relations()` accessor. `ft-cli`'s `link` / `dep`
        // commands still write the append-only `.firetrail/relations.jsonl`
        // log directly; `EmbeddedStorage::relations()` reads that same file.
        // Promoting writes through the `Storage` trait is a future follow-up.
        for rel in storage.relations()? {
            let edge = relation_to_edge(&rel);
            insert_relation(&tx, &edge)?;
            report.relations_indexed += 1;
        }

        tx.execute(
            "INSERT OR REPLACE INTO schema_meta(key, value) VALUES('last_rebuild_at', ?1);",
            params![Utc::now().to_rfc3339()],
        )?;
        if let Some(sha) = current_head_sha(&self.db_path) {
            tx.execute(
                "INSERT OR REPLACE INTO schema_meta(key, value) VALUES('last_indexed_commit', ?1);",
                params![sha],
            )?;
        }
        tx.commit()?;

        report.elapsed = start.elapsed();
        Ok(report)
    }

    /// Diff-driven refresh.
    ///
    /// `changed` is the list of on-disk paths the index should re-read.
    /// `removed` is the list of paths that no longer exist. If the combined
    /// delta exceeds the threshold (default 500) the refresh falls back to a
    /// full rebuild.
    pub fn refresh(
        &mut self,
        storage: &dyn Storage,
        changed: &[PathBuf],
        removed: &[PathBuf],
    ) -> Result<RefreshReport, IndexError> {
        if changed.len() + removed.len() > REFRESH_FULL_REBUILD_THRESHOLD {
            let rebuild = self.rebuild_from(storage)?;
            return Ok(RefreshReport {
                records_added: rebuild.records_indexed,
                records_updated: 0,
                records_removed: 0,
                elapsed: rebuild.elapsed,
            });
        }

        let start = Instant::now();
        let mut report = RefreshReport::default();

        let tx = self.conn.transaction()?;

        for path in removed {
            let path_str = path.to_string_lossy().to_string();
            let existed: Option<String> = tx
                .query_row(
                    "SELECT id FROM records WHERE file_path = ?1",
                    params![path_str],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(id) = existed {
                tx.execute("DELETE FROM records WHERE id = ?1", params![id])?;
                report.records_removed += 1;
            }
        }

        for path in changed {
            let id = id_from_path(path)?;
            let record = storage.read(&id)?;
            let existed: Option<String> = tx
                .query_row(
                    "SELECT id FROM records WHERE id = ?1",
                    params![record.envelope.id.as_str()],
                    |r| r.get(0),
                )
                .optional()?;
            // Wipe relations originating from this record so we can re-derive.
            tx.execute(
                "DELETE FROM relations WHERE from_id = ?1",
                params![record.envelope.id.as_str()],
            )?;
            insert_record(&tx, &record, path)?;
            for edge in relations_for(&record) {
                insert_relation(&tx, &edge)?;
            }
            if existed.is_some() {
                report.records_updated += 1;
            } else {
                report.records_added += 1;
            }
        }

        // Re-ingest non-structural edges on every refresh so relations added
        // since the last rebuild become visible. See `rebuild_from` for the
        // ownership note on `Storage::relations()`.
        for rel in storage.relations()? {
            let edge = relation_to_edge(&rel);
            insert_relation(&tx, &edge)?;
        }

        if let Some(sha) = current_head_sha(&self.db_path) {
            tx.execute(
                "INSERT OR REPLACE INTO schema_meta(key, value) VALUES('last_indexed_commit', ?1);",
                params![sha],
            )?;
        }
        tx.commit()?;
        report.elapsed = start.elapsed();
        Ok(report)
    }

    /// SHA of the git HEAD at the time of the most recent rebuild or refresh.
    ///
    /// Returns `None` if the workspace was never indexed inside a git repo, or
    /// if the index predates the `last_indexed_commit` `schema_meta` write.
    pub fn last_indexed_commit(&self) -> Option<String> {
        self.conn
            .query_row(
                "SELECT value FROM schema_meta WHERE key = 'last_indexed_commit'",
                [],
                |r| r.get::<_, String>(0),
            )
            .optional()
            .ok()
            .flatten()
    }

    // ── Reads ───────────────────────────────────────────────────────────────

    /// Look up one record by id.
    ///
    /// # Errors
    ///
    /// Returns [`IndexError::Integrity`] wrapping a not-found message if the
    /// record is absent (callers should fall back to direct storage reads).
    pub fn show(&self, id: &RecordId) -> Result<IndexedRecord, IndexError> {
        let mut stmt = self.conn.prepare(SELECT_RECORD_BY_ID)?;
        let row = stmt
            .query_row(params![id.as_str()], row_to_indexed)
            .optional()?
            .ok_or_else(|| IndexError::Integrity(format!("record {id} not in index")))?;
        Ok(row)
    }

    /// Run a [`ListQuery`].
    pub fn list(&self, query: &ListQuery) -> Result<Vec<IndexedRecord>, IndexError> {
        let (sql, params) = build_list_sql(query, false);
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params_from_iter(params.iter()), row_to_indexed)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Count records matching a [`ListQuery`].
    pub fn count(&self, query: &ListQuery) -> Result<u64, IndexError> {
        let (sql, params) = build_list_sql(query, true);
        let n: i64 = self
            .conn
            .query_row(&sql, params_from_iter(params.iter()), |r| r.get(0))?;
        Ok(u64::try_from(n).unwrap_or(0))
    }

    /// Records with no open blockers, neither closed nor archived nor (by
    /// default) claimed.
    pub fn ready(&self, query: &ReadyQuery) -> Result<Vec<IndexedRecord>, IndexError> {
        let (sql, params) = build_ready_sql(query);
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params_from_iter(params.iter()), row_to_indexed)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Walk the relation graph from `root` in `direction` up to `max_depth`.
    ///
    /// Cycles are detected and silently skipped — each `(from, to, kind)`
    /// triple is visited at most once. The returned edges are in
    /// breadth-first order.
    pub fn dependency_walk(
        &self,
        root: &RecordId,
        direction: WalkDirection,
        max_depth: usize,
    ) -> Result<Vec<DepEdge>, IndexError> {
        let mut out: Vec<DepEdge> = Vec::new();
        let mut seen_edges: HashSet<(String, String, String)> = HashSet::new();
        let mut seen_nodes: HashSet<String> = HashSet::new();
        let mut queue: VecDeque<(String, u32)> = VecDeque::new();

        seen_nodes.insert(root.as_str().to_string());
        queue.push_back((root.as_str().to_string(), 0));

        while let Some((cur, depth)) = queue.pop_front() {
            if depth as usize >= max_depth {
                continue;
            }
            let next_edges = step_edges(&self.conn, &cur, direction)?;
            for edge in next_edges {
                let triple = (edge.from.clone(), edge.to.clone(), edge.kind.clone());
                if !seen_edges.insert(triple) {
                    continue;
                }
                let next_node = match direction {
                    WalkDirection::Upstream => edge.to.clone(),
                    WalkDirection::Downstream => edge.from.clone(),
                    WalkDirection::Both => {
                        // Whichever endpoint we have not yet visited.
                        if edge.from == cur {
                            edge.to.clone()
                        } else {
                            edge.from.clone()
                        }
                    }
                };

                let kind = parse_relation_kind(&edge.kind)?;
                let dep_edge = DepEdge {
                    from: RecordId::from_string(edge.from.clone())
                        .map_err(|e| IndexError::Integrity(e.to_string()))?,
                    to: RecordId::from_string(edge.to.clone())
                        .map_err(|e| IndexError::Integrity(e.to_string()))?,
                    kind,
                    depth: depth + 1,
                };
                out.push(dep_edge);

                if seen_nodes.insert(next_node.clone()) {
                    queue.push_back((next_node, depth + 1));
                }
            }
        }

        Ok(out)
    }

    /// All relations directly involving `id` (in either direction).
    pub fn relations(&self, id: &RecordId) -> Result<Vec<DepEdge>, IndexError> {
        let mut stmt = self.conn.prepare(
            "SELECT from_id, to_id, kind FROM relations
             WHERE from_id = ?1 OR to_id = ?1",
        )?;
        let rows = stmt
            .query_map(params![id.as_str()], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut out = Vec::with_capacity(rows.len());
        for (from, to, kind) in rows {
            out.push(DepEdge {
                from: RecordId::from_string(from)
                    .map_err(|e| IndexError::Integrity(e.to_string()))?,
                to: RecordId::from_string(to).map_err(|e| IndexError::Integrity(e.to_string()))?,
                kind: parse_relation_kind(&kind)?,
                depth: 1,
            });
        }
        Ok(out)
    }

    /// Direct child records (via the cached `parent_id` column).
    pub fn child_records(&self, parent: &RecordId) -> Result<Vec<RecordId>, IndexError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM records WHERE parent_id = ?1")?;
        let rows = stmt
            .query_map(params![parent.as_str()], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|s| RecordId::from_string(s).map_err(|e| IndexError::Integrity(e.to_string())))
            .collect()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Insert helpers
// ─────────────────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_lines)]
fn insert_record(
    tx: &rusqlite::Transaction<'_>,
    record: &Record,
    path: &Path,
) -> Result<(), IndexError> {
    let env = &record.envelope;
    let parent_id = parent_id_of(record).map(|id| id.as_str().to_string());

    let mtime = path
        .metadata()
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0_i64, |d| i64::try_from(d.as_secs()).unwrap_or(0));

    tx.execute(
        "INSERT OR REPLACE INTO records (
            id, kind, title, status, priority, owner,
            created_by, created_at, updated_at, closed_at,
            owning_scope, state_hash, file_path, file_mtime, origin, parent_id
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6,
            ?7, ?8, ?9, ?10,
            ?11, ?12, ?13, ?14, ?15, ?16
        )",
        params![
            env.id.as_str(),
            kind_to_str(env.kind),
            env.title,
            status_to_str(env.status),
            priority_to_str(env.priority),
            env.owner.as_ref().map(Identity::as_str),
            env.created_by.as_str(),
            env.created_at.to_rfc3339(),
            env.updated_at.to_rfc3339(),
            env.closed_at.map(|t| t.to_rfc3339()),
            env.owning_scope,
            env.state_hash,
            path.to_string_lossy(),
            mtime,
            origin_to_str(env.origin),
            parent_id,
        ],
    )?;

    // Reset child tables for this id before re-inserting (covers REPLACE path).
    tx.execute(
        "DELETE FROM labels WHERE record_id = ?1",
        params![env.id.as_str()],
    )?;
    tx.execute(
        "DELETE FROM affected_scopes WHERE record_id = ?1",
        params![env.id.as_str()],
    )?;
    tx.execute(
        "DELETE FROM applies_to WHERE record_id = ?1",
        params![env.id.as_str()],
    )?;
    tx.execute(
        "DELETE FROM claims WHERE record_id = ?1",
        params![env.id.as_str()],
    )?;
    tx.execute(
        "DELETE FROM acceptance_criteria WHERE record_id = ?1",
        params![env.id.as_str()],
    )?;
    tx.execute(
        "DELETE FROM evidence WHERE record_id = ?1",
        params![env.id.as_str()],
    )?;

    for label in &env.labels {
        tx.execute(
            "INSERT OR IGNORE INTO labels(record_id, key, value) VALUES(?1, ?2, ?3)",
            params![env.id.as_str(), label.key, label.value],
        )?;
    }
    for scope in &env.affected_scopes {
        tx.execute(
            "INSERT OR IGNORE INTO affected_scopes(record_id, scope) VALUES(?1, ?2)",
            params![env.id.as_str(), scope],
        )?;
    }
    for glob in &env.applies_to {
        tx.execute(
            "INSERT OR IGNORE INTO applies_to(record_id, glob) VALUES(?1, ?2)",
            params![env.id.as_str(), glob],
        )?;
    }

    if let Some(claim) = claim_of(record) {
        tx.execute(
            "INSERT OR REPLACE INTO claims(
                record_id, claimed_by, claimed_at, claim_source, claim_expires_at
            ) VALUES(?1, ?2, ?3, ?4, ?5)",
            params![
                env.id.as_str(),
                claim.claimed_by.as_str(),
                claim.claimed_at.to_rfc3339(),
                claim.claim_source,
                claim.claim_expires_at.to_rfc3339(),
            ],
        )?;
    }

    for ac in acceptance_criteria_of(record) {
        tx.execute(
            "INSERT OR REPLACE INTO acceptance_criteria(
                id, record_id, text, status, evidence_url,
                checked_by, checked_at, created_at, updated_at, proposed
            ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                ac.id,
                env.id.as_str(),
                ac.text,
                ac_status_to_str(ac.status),
                ac.evidence_url,
                ac.checked_by.as_ref().map(Identity::as_str),
                ac.checked_at.map(|t| t.to_rfc3339()),
                ac.created_at.to_rfc3339(),
                ac.updated_at.to_rfc3339(),
                i64::from(ac.proposed),
            ],
        )?;
    }

    for ev in evidence_of(record) {
        tx.execute(
            "INSERT OR REPLACE INTO evidence(
                id, record_id, kind, url, description, created_at,
                created_by, commit_sha, symbol_name, content_hash
            ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                ev.id,
                env.id.as_str(),
                evidence_kind_to_str(ev.kind),
                ev.url,
                ev.description,
                ev.created_at.to_rfc3339(),
                ev.created_by.as_str(),
                ev.commit_sha,
                ev.symbol_name,
                ev.content_hash,
            ],
        )?;
    }

    Ok(())
}

/// A relation derived from a record's inline structural fields.
#[derive(Debug, Clone)]
struct DerivedEdge {
    from: String,
    to: String,
    kind: String,
    created_at: DateTime<Utc>,
    created_by: Identity,
}

fn relations_for(record: &Record) -> Vec<DerivedEdge> {
    let mut edges = Vec::new();
    let env = &record.envelope;
    let make_edge = |from: &str, to: &str, kind: &str| DerivedEdge {
        from: from.to_string(),
        to: to.to_string(),
        kind: kind.to_string(),
        created_at: env.created_at,
        created_by: env.created_by.clone(),
    };

    match &record.body {
        RecordBody::Task(t) => {
            if let Some(p) = &t.parent_epic {
                edges.push(make_edge(env.id.as_str(), p.as_str(), "child-of"));
                edges.push(make_edge(p.as_str(), env.id.as_str(), "parent-of"));
            }
        }
        RecordBody::Subtask(s) => {
            edges.push(make_edge(
                env.id.as_str(),
                s.parent_task.as_str(),
                "child-of",
            ));
            edges.push(make_edge(
                s.parent_task.as_str(),
                env.id.as_str(),
                "parent-of",
            ));
        }
        RecordBody::Epic(e) => {
            for child in &e.child_ids {
                edges.push(make_edge(env.id.as_str(), child.as_str(), "parent-of"));
                edges.push(make_edge(child.as_str(), env.id.as_str(), "child-of"));
            }
        }
        _ => {}
    }
    edges
}

fn insert_relation(tx: &rusqlite::Transaction<'_>, edge: &DerivedEdge) -> Result<(), IndexError> {
    tx.execute(
        "INSERT OR IGNORE INTO relations(from_id, to_id, kind, created_at, created_by)
         VALUES(?1, ?2, ?3, ?4, ?5)",
        params![
            edge.from,
            edge.to,
            edge.kind,
            edge.created_at.to_rfc3339(),
            edge.created_by.as_str(),
        ],
    )?;
    Ok(())
}

fn parent_id_of(record: &Record) -> Option<RecordId> {
    match &record.body {
        RecordBody::Task(t) => t.parent_epic.clone(),
        RecordBody::Subtask(s) => Some(s.parent_task.clone()),
        _ => None,
    }
}

fn claim_of(record: &Record) -> Option<&Claim> {
    match &record.body {
        RecordBody::Task(t) => t.claim.as_ref(),
        RecordBody::Subtask(s) => s.claim.as_ref(),
        RecordBody::Bug(b) => b.claim.as_ref(),
        _ => None,
    }
}

fn acceptance_criteria_of(record: &Record) -> &[AcceptanceCriterion] {
    match &record.body {
        RecordBody::Task(t) => &t.acceptance_criteria,
        RecordBody::Subtask(s) => &s.acceptance_criteria,
        RecordBody::Bug(b) => &b.acceptance_criteria,
        _ => &[],
    }
}

fn evidence_of(record: &Record) -> &[Evidence] {
    match &record.body {
        RecordBody::Task(t) => &t.evidence,
        RecordBody::Subtask(s) => &s.evidence,
        RecordBody::Bug(b) => &b.evidence,
        _ => &[],
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Query construction
// ─────────────────────────────────────────────────────────────────────────────

const SELECT_RECORD_BY_ID: &str = "
SELECT r.id, r.kind, r.title, r.status, r.priority, r.owner,
       r.created_by, r.created_at, r.updated_at, r.closed_at,
       r.owning_scope, r.parent_id,
       c.claimed_by, c.claimed_at, c.claim_source, c.claim_expires_at,
       (SELECT COUNT(*) FROM relations WHERE from_id = r.id AND kind = 'blocked-by') AS bb,
       (SELECT COUNT(*) FROM relations WHERE from_id = r.id AND kind = 'blocks') AS bk,
       (SELECT COUNT(*) FROM acceptance_criteria WHERE record_id = r.id) AS ct,
       (SELECT COUNT(*) FROM acceptance_criteria WHERE record_id = r.id AND status = 'checked') AS cm
FROM records r
LEFT JOIN claims c ON c.record_id = r.id
WHERE r.id = ?1
";

fn build_list_sql(q: &ListQuery, count_only: bool) -> (String, Vec<String>) {
    let select = if count_only {
        "SELECT COUNT(*) FROM records r".to_string()
    } else {
        "SELECT r.id, r.kind, r.title, r.status, r.priority, r.owner,
                r.created_by, r.created_at, r.updated_at, r.closed_at,
                r.owning_scope, r.parent_id,
                c.claimed_by, c.claimed_at, c.claim_source, c.claim_expires_at,
                (SELECT COUNT(*) FROM relations WHERE from_id = r.id AND kind = 'blocked-by') AS bb,
                (SELECT COUNT(*) FROM relations WHERE from_id = r.id AND kind = 'blocks') AS bk,
                (SELECT COUNT(*) FROM acceptance_criteria WHERE record_id = r.id) AS ct,
                (SELECT COUNT(*) FROM acceptance_criteria WHERE record_id = r.id AND status = 'checked') AS cm
         FROM records r
         LEFT JOIN claims c ON c.record_id = r.id"
            .to_string()
    };

    let mut where_clauses: Vec<String> = Vec::new();
    let mut binds: Vec<String> = Vec::new();

    if !q.include_closed {
        where_clauses.push("r.status NOT IN ('closed', 'deferred')".to_string());
    }
    if !q.include_archived {
        where_clauses.push("r.status != 'archived'".to_string());
    }

    if let Some(kinds) = &q.kinds {
        if !kinds.is_empty() {
            let placeholders = repeat_placeholders(kinds.len());
            where_clauses.push(format!("r.kind IN ({placeholders})"));
            for k in kinds {
                binds.push(kind_to_str(*k).to_string());
            }
        }
    }
    if let Some(statuses) = &q.statuses {
        if !statuses.is_empty() {
            let placeholders = repeat_placeholders(statuses.len());
            where_clauses.push(format!("r.status IN ({placeholders})"));
            for s in statuses {
                binds.push(status_to_str(*s).to_string());
            }
        }
    }
    if let Some(owners) = &q.owners {
        if !owners.is_empty() {
            let placeholders = repeat_placeholders(owners.len());
            where_clauses.push(format!("r.owner IN ({placeholders})"));
            for o in owners {
                binds.push(o.as_str().to_string());
            }
        }
    }
    if let Some(scopes) = &q.scopes {
        if !scopes.is_empty() {
            let placeholders = repeat_placeholders(scopes.len());
            where_clauses.push(format!("r.owning_scope IN ({placeholders})"));
            for s in scopes {
                binds.push(s.clone());
            }
        }
    }
    if let Some(parent) = &q.parent {
        where_clauses.push("r.parent_id = ?".to_string());
        binds.push(parent.as_str().to_string());
    }
    if let Some(since) = &q.created_since {
        where_clauses.push("r.created_at >= ?".to_string());
        binds.push(since.to_rfc3339());
    }
    if let Some(since) = &q.updated_since {
        where_clauses.push("r.updated_at >= ?".to_string());
        binds.push(since.to_rfc3339());
    }
    for (k, v) in &q.labels {
        where_clauses.push(
            "EXISTS (SELECT 1 FROM labels l WHERE l.record_id = r.id AND l.key = ? AND l.value = ?)"
                .to_string(),
        );
        binds.push(k.clone());
        binds.push(v.clone());
    }

    let mut sql = select;
    if !where_clauses.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&where_clauses.join(" AND "));
    }
    if !count_only {
        sql.push(' ');
        sql.push_str(order_by_clause(&q.order_by));
        if let Some(limit) = q.limit {
            let _ = write!(sql, " LIMIT {limit}");
        }
        if let Some(offset) = q.offset {
            let _ = write!(sql, " OFFSET {offset}");
        }
    }

    (sql, binds)
}

fn build_ready_sql(q: &ReadyQuery) -> (String, Vec<String>) {
    let mut sql = String::from(
        "SELECT r.id, r.kind, r.title, r.status, r.priority, r.owner,
                r.created_by, r.created_at, r.updated_at, r.closed_at,
                r.owning_scope, r.parent_id,
                c.claimed_by, c.claimed_at, c.claim_source, c.claim_expires_at,
                (SELECT COUNT(*) FROM relations WHERE from_id = r.id AND kind = 'blocked-by') AS bb,
                (SELECT COUNT(*) FROM relations WHERE from_id = r.id AND kind = 'blocks') AS bk,
                (SELECT COUNT(*) FROM acceptance_criteria WHERE record_id = r.id) AS ct,
                (SELECT COUNT(*) FROM acceptance_criteria WHERE record_id = r.id AND status = 'checked') AS cm
         FROM records r
         LEFT JOIN claims c ON c.record_id = r.id
         WHERE r.status NOT IN ('closed', 'deferred', 'archived')
           AND NOT EXISTS (
               SELECT 1 FROM relations rel
               INNER JOIN records blocker ON blocker.id = rel.to_id
               WHERE rel.from_id = r.id
                 AND rel.kind = 'blocked-by'
                 AND blocker.status NOT IN ('closed', 'deferred', 'archived'))",
    );

    let mut binds: Vec<String> = Vec::new();

    if !q.include_claimed {
        sql.push_str(
            " AND NOT EXISTS (
                SELECT 1 FROM claims cc
                WHERE cc.record_id = r.id
                  AND cc.claim_expires_at > ?)",
        );
        binds.push(Utc::now().to_rfc3339());
    }

    if let Some(kinds) = &q.kinds {
        if !kinds.is_empty() {
            let placeholders = repeat_placeholders(kinds.len());
            let _ = write!(sql, " AND r.kind IN ({placeholders})");
            for k in kinds {
                binds.push(kind_to_str(*k).to_string());
            }
        }
    }
    if let Some(owners) = &q.owners {
        if !owners.is_empty() {
            let placeholders = repeat_placeholders(owners.len());
            let _ = write!(sql, " AND r.owner IN ({placeholders})");
            for o in owners {
                binds.push(o.as_str().to_string());
            }
        }
    }
    if let Some(scopes) = &q.scopes {
        if !scopes.is_empty() {
            let placeholders = repeat_placeholders(scopes.len());
            let _ = write!(sql, " AND r.owning_scope IN ({placeholders})");
            for s in scopes {
                binds.push(s.clone());
            }
        }
    }

    sql.push_str(" ORDER BY r.priority ASC, r.updated_at DESC");
    if let Some(limit) = q.limit {
        let _ = write!(sql, " LIMIT {limit}");
    }

    (sql, binds)
}

fn repeat_placeholders(n: usize) -> String {
    std::iter::repeat_n("?", n).collect::<Vec<_>>().join(", ")
}

fn order_by_clause(order: &OrderBy) -> &'static str {
    match order {
        OrderBy::Priority => "ORDER BY r.priority ASC, r.updated_at DESC",
        OrderBy::CreatedAt => "ORDER BY r.created_at DESC",
        OrderBy::UpdatedAt => "ORDER BY r.updated_at DESC",
        OrderBy::Title => "ORDER BY r.title ASC",
    }
}

fn step_edges(
    conn: &Connection,
    from: &str,
    direction: WalkDirection,
) -> Result<Vec<RelRow>, IndexError> {
    let (sql, bind): (&str, Vec<&str>) = match direction {
        WalkDirection::Upstream => (
            "SELECT from_id, to_id, kind FROM relations
             WHERE from_id = ?1 AND kind = 'blocked-by'",
            vec![from],
        ),
        WalkDirection::Downstream => (
            "SELECT from_id, to_id, kind FROM relations
             WHERE to_id = ?1 AND kind = 'blocked-by'",
            vec![from],
        ),
        WalkDirection::Both => (
            "SELECT from_id, to_id, kind FROM relations
             WHERE (from_id = ?1 OR to_id = ?1)
               AND kind IN ('blocked-by', 'blocks', 'parent-of', 'child-of')",
            vec![from],
        ),
    };
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt
        .query_map(params_from_iter(bind.iter()), |r| {
            Ok(RelRow {
                from: r.get(0)?,
                to: r.get(1)?,
                kind: r.get(2)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

#[derive(Debug, Clone)]
struct RelRow {
    from: String,
    to: String,
    kind: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// Row mapping
// ─────────────────────────────────────────────────────────────────────────────

fn row_to_indexed(row: &rusqlite::Row<'_>) -> rusqlite::Result<IndexedRecord> {
    let id_s: String = row.get(0)?;
    let kind_s: String = row.get(1)?;
    let title: String = row.get(2)?;
    let status_s: String = row.get(3)?;
    let priority_s: String = row.get(4)?;
    let owner_s: Option<String> = row.get(5)?;
    let created_by_s: String = row.get(6)?;
    let created_at_s: String = row.get(7)?;
    let updated_at_s: String = row.get(8)?;
    let closed_at_s: Option<String> = row.get(9)?;
    let owning_scope: Option<String> = row.get(10)?;
    let parent_id_s: Option<String> = row.get(11)?;

    let claimed_by_s: Option<String> = row.get(12)?;
    let claimed_at_s: Option<String> = row.get(13)?;
    let claim_source: Option<String> = row.get(14)?;
    let claim_expires_s: Option<String> = row.get(15)?;
    let bb: i64 = row.get(16)?;
    let bk: i64 = row.get(17)?;
    let ct: i64 = row.get(18)?;
    let cm: i64 = row.get(19)?;

    let map_err = |e: String| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, e.into())
    };

    let id = RecordId::from_string(id_s).map_err(|e| map_err(e.to_string()))?;
    let kind = kind_from_str(&kind_s).map_err(map_err)?;
    let status = status_from_str(&status_s).map_err(map_err)?;
    let priority = priority_from_str(&priority_s).map_err(map_err)?;
    let owner = owner_s
        .map(Identity::new)
        .transpose()
        .map_err(|e| map_err(e.to_string()))?;
    let created_by = Identity::new(created_by_s).map_err(|e| map_err(e.to_string()))?;
    let created_at = parse_dt(&created_at_s).map_err(map_err)?;
    let updated_at = parse_dt(&updated_at_s).map_err(map_err)?;
    let closed_at = closed_at_s
        .map(|s| parse_dt(&s))
        .transpose()
        .map_err(map_err)?;
    let parent_id = parent_id_s
        .map(RecordId::from_string)
        .transpose()
        .map_err(|e| map_err(e.to_string()))?;

    let claim = match (claimed_by_s, claimed_at_s, claim_source, claim_expires_s) {
        (Some(by), Some(at), Some(src), Some(exp)) => Some(Claim {
            claimed_by: Identity::new(by).map_err(|e| map_err(e.to_string()))?,
            claimed_at: parse_dt(&at).map_err(map_err)?,
            claim_source: src,
            claim_expires_at: parse_dt(&exp).map_err(map_err)?,
        }),
        _ => None,
    };

    Ok(IndexedRecord {
        id,
        kind,
        title,
        status,
        priority,
        owner,
        created_by,
        created_at,
        updated_at,
        closed_at,
        owning_scope,
        claim,
        blocked_by_count: u32::try_from(bb).unwrap_or(0),
        blocks_count: u32::try_from(bk).unwrap_or(0),
        parent_id,
        criteria_total: u32::try_from(ct).unwrap_or(0),
        criteria_met: u32::try_from(cm).unwrap_or(0),
    })
}

fn parse_dt(s: &str) -> Result<DateTime<Utc>, String> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| format!("parse datetime `{s}`: {e}"))
}

// ─────────────────────────────────────────────────────────────────────────────
// Enum ↔ string helpers (kept here, not on ft-core enums, to avoid coupling
// the wire format to internal SQL column conventions)
// ─────────────────────────────────────────────────────────────────────────────

fn kind_to_str(k: RecordKind) -> &'static str {
    match k {
        RecordKind::Epic => "epic",
        RecordKind::Task => "task",
        RecordKind::Subtask => "subtask",
        RecordKind::Bug => "bug",
        RecordKind::Incident => "incident",
        RecordKind::Finding => "finding",
        RecordKind::Runbook => "runbook",
        RecordKind::Decision => "decision",
        RecordKind::Gotcha => "gotcha",
        RecordKind::Memory => "memory",
        RecordKind::Doc => "doc",
        RecordKind::RepoProfile => "repo_profile",
    }
}

fn kind_from_str(s: &str) -> Result<RecordKind, String> {
    Ok(match s {
        "epic" => RecordKind::Epic,
        "task" => RecordKind::Task,
        "subtask" => RecordKind::Subtask,
        "bug" => RecordKind::Bug,
        "incident" => RecordKind::Incident,
        "finding" => RecordKind::Finding,
        "runbook" => RecordKind::Runbook,
        "decision" => RecordKind::Decision,
        "gotcha" => RecordKind::Gotcha,
        "memory" => RecordKind::Memory,
        "doc" => RecordKind::Doc,
        "repo_profile" => RecordKind::RepoProfile,
        other => return Err(format!("unknown kind `{other}`")),
    })
}

fn status_to_str(s: Status) -> &'static str {
    match s {
        Status::Open => "open",
        Status::Ready => "ready",
        Status::InProgress => "in_progress",
        Status::Review => "review",
        Status::Blocked => "blocked",
        Status::Closed => "closed",
        Status::Deferred => "deferred",
        Status::Archived => "archived",
    }
}

fn status_from_str(s: &str) -> Result<Status, String> {
    Ok(match s {
        "open" => Status::Open,
        "ready" => Status::Ready,
        "in_progress" => Status::InProgress,
        "review" => Status::Review,
        "blocked" => Status::Blocked,
        "closed" => Status::Closed,
        "deferred" => Status::Deferred,
        "archived" => Status::Archived,
        other => return Err(format!("unknown status `{other}`")),
    })
}

fn priority_to_str(p: Priority) -> &'static str {
    match p {
        Priority::P0 => "p0",
        Priority::P1 => "p1",
        Priority::P2 => "p2",
        Priority::P3 => "p3",
        Priority::P4 => "p4",
    }
}

fn priority_from_str(s: &str) -> Result<Priority, String> {
    Ok(match s {
        "p0" => Priority::P0,
        "p1" => Priority::P1,
        "p2" => Priority::P2,
        "p3" => Priority::P3,
        "p4" => Priority::P4,
        other => return Err(format!("unknown priority `{other}`")),
    })
}

fn origin_to_str(o: Origin) -> &'static str {
    match o {
        Origin::Human => "human",
        Origin::Agent => "agent",
        Origin::Imported => "imported",
    }
}

fn ac_status_to_str(s: AcStatus) -> &'static str {
    match s {
        AcStatus::Unchecked => "unchecked",
        AcStatus::Checked => "checked",
    }
}

fn evidence_kind_to_str(k: EvidenceKind) -> &'static str {
    match k {
        EvidenceKind::IncidentReport => "incident_report",
        EvidenceKind::PullRequest => "pull_request",
        EvidenceKind::Commit => "commit",
        EvidenceKind::Dashboard => "dashboard",
        EvidenceKind::LogQuery => "log_query",
        EvidenceKind::TestResult => "test_result",
        EvidenceKind::JiraTicket => "jira_ticket",
        EvidenceKind::ConfluencePage => "confluence_page",
        EvidenceKind::ManualNote => "manual_note",
    }
}

/// Recover a [`RecordId`] from the on-disk filename a Storage `path_for` would
/// produce.
///
/// The file layout is `<root>/.firetrail/records/<kind>/<lower-id>.json`; the
/// canonical id is `<KIND-PREFIX>-<hex>` and the stem matches the lowercased
/// id one-to-one.
fn id_from_path(path: &Path) -> Result<RecordId, IndexError> {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| IndexError::Integrity(format!("bad path: {}", path.display())))?;
    let (prefix, rest) = stem.split_once('-').ok_or_else(|| {
        IndexError::Integrity(format!("cannot parse id from path: {}", path.display()))
    })?;
    let canonical = format!("{}-{}", prefix.to_uppercase(), rest.to_lowercase());
    RecordId::from_string(canonical).map_err(|e| IndexError::Integrity(e.to_string()))
}

/// Resolve the git HEAD SHA for the workspace this index lives in.
///
/// `db_path` is `<workspace_root>/.firetrail/index.db`. Walks two levels up
/// and tries to open the workspace as a git repo. Returns `None` if the index
/// is not inside a git repo or the head cannot be resolved — populating
/// `last_indexed_commit` is best-effort.
fn current_head_sha(db_path: &Path) -> Option<String> {
    let root = db_path.parent()?.parent()?;
    let repo = ft_git::Repo::open(root.to_path_buf()).ok()?;
    repo.head().ok().map(|h| h.commit_sha)
}

/// Convert a [`ft_core::Relation`] (as returned by `Storage::relations()`)
/// into the internal [`DerivedEdge`] representation used by
/// [`insert_relation`].
fn relation_to_edge(rel: &ft_core::Relation) -> DerivedEdge {
    DerivedEdge {
        from: rel.from.as_str().to_string(),
        to: rel.to.as_str().to_string(),
        kind: relation_kind_to_str(rel.kind).to_string(),
        created_at: rel.created_at,
        created_by: rel.created_by.clone(),
    }
}

fn relation_kind_to_str(k: RelationKind) -> &'static str {
    match k {
        RelationKind::Blocks => "blocks",
        RelationKind::BlockedBy => "blocked-by",
        RelationKind::ParentOf => "parent-of",
        RelationKind::ChildOf => "child-of",
        RelationKind::RelatedTo => "related-to",
        RelationKind::Duplicates => "duplicates",
        RelationKind::Supersedes => "supersedes",
        RelationKind::DiscoveredDuring => "discovered-during",
        RelationKind::FollowUpFrom => "follow-up-from",
        RelationKind::FixedBy => "fixed-by",
        RelationKind::CausedBy => "caused-by",
        RelationKind::MitigatedBy => "mitigated-by",
        RelationKind::DocumentedIn => "documented-in",
        RelationKind::ImplementedBy => "implemented-by",
        RelationKind::RegressedBy => "regressed-by",
        RelationKind::Affects => "affects",
        RelationKind::OwnedBy => "owned-by",
    }
}

fn parse_relation_kind(s: &str) -> Result<RelationKind, IndexError> {
    Ok(match s {
        "blocks" => RelationKind::Blocks,
        "blocked-by" => RelationKind::BlockedBy,
        "parent-of" => RelationKind::ParentOf,
        "child-of" => RelationKind::ChildOf,
        "related-to" => RelationKind::RelatedTo,
        "duplicates" => RelationKind::Duplicates,
        "supersedes" => RelationKind::Supersedes,
        "discovered-during" => RelationKind::DiscoveredDuring,
        "follow-up-from" => RelationKind::FollowUpFrom,
        "fixed-by" => RelationKind::FixedBy,
        "caused-by" => RelationKind::CausedBy,
        "mitigated-by" => RelationKind::MitigatedBy,
        "documented-in" => RelationKind::DocumentedIn,
        "implemented-by" => RelationKind::ImplementedBy,
        "regressed-by" => RelationKind::RegressedBy,
        "affects" => RelationKind::Affects,
        "owned-by" => RelationKind::OwnedBy,
        other => {
            return Err(IndexError::Integrity(format!(
                "unknown relation kind `{other}`"
            )));
        }
    })
}

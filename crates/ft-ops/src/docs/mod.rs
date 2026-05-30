//! Transport-agnostic doc surface — file-backed documentation linked to work
//! items (firetrail-2mwp).
//!
//! The `.md` file on disk is the single source of truth; a [`ft_core::Doc`]
//! record is a thin pointer carrying `path` + `content_hash` for drift
//! detection. These ops back the ft-ui ticket-drawer Docs panel:
//!
//! - [`docs_for_ticket`] — every `DocumentedIn` doc for a ticket, rendered
//!   (raw markdown) with a [`DocFreshnessView`] computed from `content_hash`.
//! - [`edit`] — write new content through to the file, re-derive the hash +
//!   summary, persist, and re-index synchronously (the watcher-free path the
//!   docs design calls for).
//! - [`add`] / [`link`] — adopt an existing `.md` as a `Doc` and connect it to
//!   a work item. These mirror `firetrail doc add/link` for embedded
//!   workspaces; the CLI keeps its own external-storage-aware bodies and shares
//!   only the pure [`parse_doc_meta`] / [`apply_doc_content`] derivation.
//!
//! Like every op in this crate these are embedded-storage only and take
//! `(&Workspace, &Identity, Input, &EventBus)`.

use std::collections::HashSet;
use std::path::Path;

use chrono::Utc;
use ft_core::{Doc, RecordBody, RecordBuilder, RecordId, RecordKind, Relation, RelationKind};
use ft_embed::{apply_doc_content, parse_doc_meta, parse_frontmatter};
use serde::{Deserialize, Serialize};

use crate::error::OpsError;
use crate::events::{Event, EventBus};
use crate::identity::Identity;
use crate::tickets::ctx::{TicketCtx, append_relation, load_relations};
use crate::workspace::Workspace;

// ─────────────────────────────────────────────────────────────────────────────
// Wire types.
// ─────────────────────────────────────────────────────────────────────────────

/// Freshness of a linked doc's file relative to its indexed `content_hash`.
///
/// The wire mirror of [`ft_embed::DocFreshness`] — kept in `ft-ops` so the
/// embed crate stays serialization-free and ts-rs only ever sees ops types.
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DocFreshnessView {
    /// File hash matches the record — search/render are current.
    Fresh,
    /// File changed since last index — needs a re-index (still rendered).
    Stale,
    /// Linked file missing/unreadable — a broken link.
    Missing,
}

impl From<ft_embed::DocFreshness> for DocFreshnessView {
    fn from(f: ft_embed::DocFreshness) -> Self {
        match f {
            ft_embed::DocFreshness::Fresh => Self::Fresh,
            ft_embed::DocFreshness::Stale => Self::Stale,
            ft_embed::DocFreshness::Missing => Self::Missing,
        }
    }
}

/// A linked doc as the ticket drawer renders it: pointer metadata + the raw
/// markdown content read live from the file + a freshness badge.
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DocView {
    /// Canonical doc record id.
    pub id: String,
    /// Title (derived from the doc's H1, mirrored on the record).
    pub title: String,
    /// Open taxonomy tag (`design` / `adr` / `runbook` / …).
    pub doc_type: String,
    /// Repo-relative path to the backing `.md` file.
    pub path: String,
    /// Short excerpt stored on the record (first prose paragraph).
    pub summary: String,
    /// Freshness of the file relative to the indexed `content_hash`.
    pub freshness: DocFreshnessView,
    /// Raw markdown read live from the file. Empty when [`DocFreshnessView::Missing`].
    pub content: String,
}

/// Input for [`add`] — adopt an existing `.md` file as a `Doc` record.
///
/// `file` must already be **repo-relative**: ft-ops never touches ambient
/// process state, so any cwd→repo resolution happens in the adapter.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AddDocInput {
    /// Repo-relative path to the markdown file.
    pub file: String,
    /// Open taxonomy tag stored on the record.
    pub doc_type: String,
    /// Optional explicit title (falls back to the doc H1, then the file stem).
    pub title: Option<String>,
    /// Optional owning scope for the envelope.
    pub scope: Option<String>,
}

/// Result of [`add`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AddDocOutput {
    /// New doc record id.
    pub id: String,
    /// Repo-relative path adopted.
    pub path: String,
}

/// Input for [`link`] — connect a doc to a work item via `DocumentedIn`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LinkDocInput {
    /// Doc record id (full or unambiguous prefix).
    pub doc: String,
    /// Work item (task/epic) id the doc documents.
    pub work_item: String,
}

/// Result of [`link`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LinkDocOutput {
    /// Resolved work item id.
    pub work_item: String,
    /// Resolved doc id.
    pub doc: String,
}

/// Input for [`edit`] — write new content through to the file + re-index.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EditDocInput {
    /// Doc record id (full or unambiguous prefix).
    pub id: String,
    /// New full markdown content to write to the backing file.
    pub content: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// Read.
// ─────────────────────────────────────────────────────────────────────────────

/// Every `DocumentedIn` doc for `ticket_id`, rendered with a freshness badge.
///
/// Read-only: it computes freshness by hashing each file live but does **not**
/// rewrite any `content_hash` (that needs an actor — see [`edit`]). Records
/// that resolve but aren't docs, and edges whose target was deleted, are
/// silently skipped; a missing *file* surfaces as [`DocFreshnessView::Missing`]
/// so a broken link is visible rather than dropped.
#[allow(clippy::needless_pass_by_value)]
pub fn docs_for_ticket(
    ws: &Workspace,
    identity: &Identity,
    ticket_id: String,
    _events: &EventBus,
) -> Result<Vec<DocView>, OpsError> {
    let ctx = TicketCtx::open(ws, identity, "doc list")?;
    let ticket = ctx.resolve_id(&ticket_id)?;

    let relations = load_relations(ws)?;
    let mut views = Vec::new();
    for rel in relations
        .iter()
        .filter(|r| r.kind == RelationKind::DocumentedIn && r.from == ticket)
    {
        let Ok(record) = ctx.read_record(&rel.to) else {
            continue; // edge points at a deleted record — skip.
        };
        let RecordBody::Doc(doc) = &record.body else {
            continue; // not a doc (malformed edge) — skip.
        };
        let freshness = ft_embed::doc_freshness(&ws.root, doc);
        let content = std::fs::read_to_string(ws.root.join(&doc.path)).unwrap_or_default();
        views.push(DocView {
            id: record.envelope.id.as_str().to_string(),
            title: doc.title.clone(),
            doc_type: doc.doc_type.clone(),
            path: doc.path.clone(),
            summary: doc.summary.clone(),
            freshness: freshness.into(),
            content,
        });
    }
    Ok(views)
}

// ─────────────────────────────────────────────────────────────────────────────
// Write-through edit.
// ─────────────────────────────────────────────────────────────────────────────

/// Write `input.content` to the doc's file, then re-derive `content_hash` +
/// `summary` and persist — the synchronous, watcher-free re-index path.
///
/// Returns the refreshed [`DocView`]: after a successful write the file and the
/// record agree, so `freshness` is [`DocFreshnessView::Fresh`].
#[allow(clippy::needless_pass_by_value)]
pub fn edit(
    ws: &Workspace,
    identity: &Identity,
    input: EditDocInput,
    events: &EventBus,
) -> Result<DocView, OpsError> {
    let mut ctx = TicketCtx::open(ws, identity, "doc edit")?;
    let id = ctx.resolve_id(&input.id)?;
    let mut record = ctx.read_record(&id)?;
    let path = match &record.body {
        RecordBody::Doc(doc) => doc.path.clone(),
        _ => {
            return Err(OpsError::validation(
                "id",
                format!("{id} is not a doc record"),
            ));
        }
    };

    let abs = ws.root.join(&path);
    std::fs::write(&abs, &input.content).map_err(|e| {
        OpsError::Internal(anyhow::anyhow!("write doc file {}: {e}", abs.display()))
    })?;

    // Re-derive hash + summary from the new content; persist only on change.
    if apply_doc_content(&mut record, &input.content) {
        ctx.save_record(&mut record)?;
    }

    let RecordBody::Doc(doc) = &record.body else {
        unreachable!("body was a Doc above");
    };

    // Notify other connected clients so they re-fetch the doc list and the
    // freshness badge flips stale → fresh without a manual reload.
    events.emit(Event::DocEdited {
        id: record.envelope.id.as_str().to_string(),
    });

    let freshness = ft_embed::doc_freshness(&ws.root, doc);
    Ok(DocView {
        id: record.envelope.id.as_str().to_string(),
        title: doc.title.clone(),
        doc_type: doc.doc_type.clone(),
        path: doc.path.clone(),
        summary: doc.summary.clone(),
        freshness: freshness.into(),
        content: input.content,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Adopt + link.
// ─────────────────────────────────────────────────────────────────────────────

/// Adopt an existing repo-relative `.md` file as a `Doc` record.
#[allow(clippy::needless_pass_by_value)]
pub fn add(
    ws: &Workspace,
    identity: &Identity,
    input: AddDocInput,
    _events: &EventBus,
) -> Result<AddDocOutput, OpsError> {
    let mut ctx = TicketCtx::open(ws, identity, "doc add")?;
    let abs = ws.root.join(&input.file);
    let content = std::fs::read_to_string(&abs)
        .map_err(|e| OpsError::validation("file", format!("cannot read {}: {e}", input.file)))?;

    // Spec §5 frontmatter is authoritative where present: `doc_type` and
    // `scope` override the call inputs (mirroring each other's precedence),
    // `status` seeds the trust state, and `links:` becomes DocumentedIn edges.
    let fm = parse_frontmatter(&content);
    let (parsed_title, summary) = parse_doc_meta(&content);
    let title = input
        .title
        .or(parsed_title)
        .unwrap_or_else(|| file_stem(&input.file));
    let actor = ctx.actor.clone();
    let doc_type = fm.doc_type.unwrap_or(input.doc_type);
    let trust = fm.status.unwrap_or(ft_core::TrustState::Draft);
    let scope = fm.scope.or(input.scope);

    let mut builder = RecordBuilder::new(RecordKind::Doc, &title, actor).doc(Doc {
        path: input.file.clone(),
        content_hash: ft_embed::content_hash(&content),
        title: title.clone(),
        summary,
        doc_type,
        trust,
    });
    if let Some(scope) = &scope {
        builder = builder.owning_scope(scope);
    }
    let mut record = builder
        .build()
        .map_err(|e| OpsError::Internal(anyhow::anyhow!("build doc: {e}")))?;
    ctx.save_record(&mut record)?;
    let doc_id = record.envelope.id.clone();

    // Adopt each frontmatter `links:` work item as a `work_item
    // --DocumentedIn--> doc` edge. Unresolvable ids, self-links, and
    // duplicates are skipped silently — a forgiving convention, not a gate.
    let mut seen: HashSet<RecordId> = HashSet::new();
    let mut linked_any = false;
    for raw in &fm.links {
        let Ok(work_id) = ctx.resolve_id(raw) else {
            continue;
        };
        if work_id == doc_id || !seen.insert(work_id.clone()) {
            continue;
        }
        append_relation(
            ws,
            &Relation {
                from: work_id,
                to: doc_id.clone(),
                kind: RelationKind::DocumentedIn,
                created_at: Utc::now(),
                created_by: ctx.actor.clone(),
            },
        )?;
        linked_any = true;
    }
    if linked_any {
        ctx.index
            .refresh(&ctx.storage, &[], &[])
            .map_err(|e| OpsError::Internal(anyhow::anyhow!("index refresh: {e}")))?;
    }

    Ok(AddDocOutput {
        id: doc_id.as_str().to_string(),
        path: input.file,
    })
}

/// Record a `work_item --DocumentedIn--> doc` edge.
#[allow(clippy::needless_pass_by_value)]
pub fn link(
    ws: &Workspace,
    identity: &Identity,
    input: LinkDocInput,
    _events: &EventBus,
) -> Result<LinkDocOutput, OpsError> {
    let mut ctx = TicketCtx::open(ws, identity, "doc link")?;
    let doc_id = ctx.resolve_id(&input.doc)?;
    let work_id = ctx.resolve_id(&input.work_item)?;
    if doc_id == work_id {
        return Err(OpsError::validation(
            "work_item",
            "cannot link a record to itself",
        ));
    }

    let doc_rec = ctx.read_record(&doc_id)?;
    if !matches!(doc_rec.body, RecordBody::Doc(_)) {
        return Err(OpsError::validation(
            "doc",
            format!("{doc_id} is not a doc record"),
        ));
    }
    let _ = ctx.read_record(&work_id)?;

    let relation = Relation {
        from: work_id.clone(),
        to: doc_id.clone(),
        kind: RelationKind::DocumentedIn,
        created_at: Utc::now(),
        created_by: ctx.actor.clone(),
    };
    append_relation(ws, &relation)?;
    ctx.index
        .refresh(&ctx.storage, &[], &[])
        .map_err(|e| OpsError::Internal(anyhow::anyhow!("index refresh: {e}")))?;

    Ok(LinkDocOutput {
        work_item: work_id.as_str().to_string(),
        doc: doc_id.as_str().to_string(),
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers.
// ─────────────────────────────────────────────────────────────────────────────

fn file_stem(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .map_or_else(|| path.to_string(), |s| s.to_string_lossy().into_owned())
}

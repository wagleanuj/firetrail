//! `firetrail doc {add,link,index}` — manage file-backed documentation records.
//!
//! A `Doc` record points at an external `.md` file (the file is the source of
//! truth). `add` adopts an existing file into a record; `link` connects a doc
//! to a work item via a `DocumentedIn` relation so `prime` delivers it; `index`
//! re-reads the file(s) and refreshes the stored `content_hash`/summary +
//! search index after out-of-band edits.

use std::collections::HashSet;
use std::path::Path;

use chrono::Utc;
use ft_core::{Doc, RecordBody, RecordBuilder, RecordId, RecordKind, Relation, RelationKind};
use ft_embed::{apply_doc_content, apply_doc_frontmatter, parse_doc_meta, parse_frontmatter};
use ft_storage::{Storage as _, StorageFilter};
use serde::Serialize;

use crate::cli::{DocAddArgs, DocIndexArgs, DocLinkArgs, GlobalOpts};
use crate::commands::CommandOutcome;
use crate::context::{WorkCtx, append_relation, load_relations};
use crate::error::CliError;

const ADD: &str = "doc add";
const LINK: &str = "doc link";
const INDEX: &str = "doc index";

/// Outcome of any `doc` subcommand.
#[derive(Debug, Clone, Serialize)]
pub struct DocOutcome {
    #[serde(skip)]
    pub command: &'static str,
    /// Action label (`add` / `link` / `index`).
    pub action: &'static str,
    /// Human-readable summary line.
    pub message: String,
    /// Affected record id(s).
    pub ids: Vec<String>,
    /// Non-fatal warnings.
    pub warnings: Vec<String>,
}

impl DocOutcome {
    pub fn markdown(&self) -> String {
        use std::fmt::Write as _;
        let mut s = format!("**doc {}** — {}", self.action, self.message);
        for id in &self.ids {
            let _ = write!(s, "\n- `{id}`");
        }
        s
    }
    pub fn quiet_line(&self) -> String {
        self.ids.join(" ")
    }
}

/// `firetrail doc add <file> --type <t>` — adopt an existing markdown file.
pub fn add(args: &DocAddArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let mut ctx = WorkCtx::open(ADD, global.workspace.as_deref())?;

    let rel_path = repo_relative(&ctx.ws.root, &args.file)
        .ok_or_else(|| CliError::user(ADD, "file must live inside the workspace"))?;
    let abs = ctx.ws.root.join(&rel_path);
    let content = std::fs::read_to_string(&abs)
        .map_err(|e| CliError::user(ADD, format!("cannot read {}: {e}", abs.display())))?;

    // Spec §5 frontmatter is authoritative where present: `doc_type` and
    // `scope` override the flags, `status` seeds trust, and `links:` becomes
    // DocumentedIn edges (mirrors `ft_ops::docs::add`).
    let fm = parse_frontmatter(&content);
    let (parsed_title, summary) = parse_doc_meta(&content);
    let title = args
        .title
        .clone()
        .or(parsed_title)
        .unwrap_or_else(|| file_stem(&rel_path));
    let created_by = ctx.actor()?;
    let doc_type = fm.doc_type.unwrap_or_else(|| args.doc_type.clone());
    let trust = fm.status.unwrap_or(ft_core::TrustState::Draft);
    let scope = fm.scope.or_else(|| args.scope.clone());

    let mut builder = RecordBuilder::new(RecordKind::Doc, &title, created_by.clone()).doc(Doc {
        path: rel_path.clone(),
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
        .map_err(|e| CliError::internal(ADD, format!("build doc: {e}")))?;
    ctx.save_record(&mut record)?;
    let doc_id = record.envelope.id.clone();

    // Adopt each frontmatter `links:` work item as a `work_item
    // --DocumentedIn--> doc` edge; skip self-links, duplicates, and
    // unresolvable ids silently.
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
            &ctx.ws,
            &Relation {
                from: work_id,
                to: doc_id.clone(),
                kind: RelationKind::DocumentedIn,
                created_at: Utc::now(),
                created_by: created_by.clone(),
            },
        )?;
        linked_any = true;
    }
    if linked_any {
        ctx.index
            .refresh(&ctx.storage, &[], &[])
            .map_err(|e| CliError::internal(ADD, format!("index refresh: {e}")))?;
    }

    let id = doc_id.as_str().to_string();
    Ok(CommandOutcome::Doc(DocOutcome {
        command: ADD,
        action: "add",
        message: format!("adopted {rel_path} as {}", &id),
        ids: vec![id],
        warnings: ctx.warnings.clone(),
    }))
}

/// `firetrail doc link <doc> <work-item>` — record a `DocumentedIn` edge so
/// priming the work item delivers the doc.
pub fn link(args: &DocLinkArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let mut ctx = WorkCtx::open(LINK, global.workspace.as_deref())?;
    let doc_id = ctx.resolve_id(&args.doc)?;
    let work_id = ctx.resolve_id(&args.work_item)?;
    if doc_id == work_id {
        return Err(CliError::user(LINK, "cannot link a record to itself"));
    }

    // Endpoints must exist, and `doc` must actually be a Doc.
    let doc_rec = ctx.read_record(&doc_id)?;
    if !matches!(doc_rec.body, RecordBody::Doc(_)) {
        return Err(CliError::user(
            LINK,
            format!("{doc_id} is not a doc record"),
        ));
    }
    let _ = ctx.read_record(&work_id)?;

    // work-item --documented-in--> doc
    let relation = Relation {
        from: work_id.clone(),
        to: doc_id.clone(),
        kind: RelationKind::DocumentedIn,
        created_at: Utc::now(),
        created_by: ctx.actor()?,
    };
    append_relation(&ctx.ws, &relation)?;
    ctx.index
        .refresh(&ctx.storage, &[], &[])
        .map_err(|e| CliError::internal(LINK, format!("index refresh: {e}")))?;

    Ok(CommandOutcome::Doc(DocOutcome {
        command: LINK,
        action: "link",
        message: format!("{work_id} documented-in {doc_id}"),
        ids: vec![work_id.as_str().into(), doc_id.as_str().into()],
        warnings: ctx.warnings.clone(),
    }))
}

/// `firetrail doc index [target]` — re-read the file(s) and refresh the stored
/// `content_hash`/summary + search index after out-of-band edits. With no
/// target, every doc record is checked; only changed ones are rewritten.
pub fn index(args: &DocIndexArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let mut ctx = WorkCtx::open(INDEX, global.workspace.as_deref())?;

    let targets: Vec<RecordId> = if let Some(raw) = &args.target {
        vec![ctx.resolve_id(raw)?]
    } else {
        ctx.storage
            .iter(&StorageFilter::default().kind(RecordKind::Doc))
            .filter_map(Result::ok)
            .map(|r| r.envelope.id)
            .collect()
    };

    let root = ctx.ws.root.clone();
    let mut refreshed = Vec::new();
    let mut warnings = ctx.warnings.clone();
    let mut linked_any = false;
    for id in targets {
        let record = ctx.read_record(&id)?;
        let RecordBody::Doc(doc) = &record.body else {
            warnings.push(format!("{id} is not a doc record — skipped"));
            continue;
        };
        let doc_path = doc.path.clone();
        match std::fs::read_to_string(root.join(&doc_path)) {
            Ok(content) => {
                // Refresh hash + summary AND §5 frontmatter (`doc_type` body +
                // `owning_scope` envelope); persist if either changed. Per the
                // approved Option C, `trust` is NOT refreshed on re-index —
                // frontmatter `status:` is consumed only at `doc add`.
                let mut updated = record.clone();
                let content_changed = apply_doc_content(&mut updated, &content);
                let fm_changed = apply_doc_frontmatter(&mut updated, &content);
                if content_changed || fm_changed {
                    ctx.save_record(&mut updated)?;
                    refreshed.push(id.as_str().to_string());
                }

                // Additively reconcile `links:` → DocumentedIn edges (same rule
                // as `doc add`): create missing edges, never remove existing
                // ones (they may have come from explicit `doc link`).
                let existing = load_relations(&ctx.ws)?;
                let mut seen: HashSet<RecordId> = existing
                    .iter()
                    .filter(|r| r.kind == RelationKind::DocumentedIn && r.to == id)
                    .map(|r| r.from.clone())
                    .collect();
                let fm = parse_frontmatter(&content);
                let created_by = ctx.actor()?;
                for raw in &fm.links {
                    let Ok(work_id) = ctx.resolve_id(raw) else {
                        continue;
                    };
                    if work_id == id || !seen.insert(work_id.clone()) {
                        continue;
                    }
                    append_relation(
                        &ctx.ws,
                        &Relation {
                            from: work_id,
                            to: id.clone(),
                            kind: RelationKind::DocumentedIn,
                            created_at: Utc::now(),
                            created_by: created_by.clone(),
                        },
                    )?;
                    linked_any = true;
                }
            }
            Err(_) => warnings.push(format!(
                "doc {id} points at a missing file ({doc_path}) — broken link"
            )),
        }
    }
    if linked_any {
        ctx.index
            .refresh(&ctx.storage, &[], &[])
            .map_err(|e| CliError::internal(INDEX, format!("index refresh: {e}")))?;
    }

    Ok(CommandOutcome::Doc(DocOutcome {
        command: INDEX,
        action: "index",
        message: format!("{} doc(s) refreshed from file", refreshed.len()),
        ids: refreshed,
        warnings,
    }))
}

// ─── helpers ─────────────────────────────────────────────────────────────────

/// Make `file` relative to the workspace `root`. Returns `None` if it escapes.
fn repo_relative(root: &Path, file: &str) -> Option<String> {
    let p = Path::new(file);
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir().ok()?.join(p)
    };
    let canon_root = root.canonicalize().ok()?;
    let canon = abs.canonicalize().ok()?;
    canon
        .strip_prefix(&canon_root)
        .ok()
        .map(|r| r.to_string_lossy().replace('\\', "/"))
}

fn file_stem(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .map_or_else(|| path.to_string(), |s| s.to_string_lossy().into_owned())
}

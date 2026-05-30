//! `firetrail doc {add,link,index}` — manage file-backed documentation records.
//!
//! A `Doc` record points at an external `.md` file (the file is the source of
//! truth). `add` adopts an existing file into a record; `link` connects a doc
//! to a work item via a `DocumentedIn` relation so `prime` delivers it; `index`
//! re-reads the file(s) and refreshes the stored `content_hash`/summary +
//! search index after out-of-band edits.

use std::path::Path;

use chrono::Utc;
use ft_core::{Doc, RecordBody, RecordBuilder, RecordId, RecordKind, Relation, RelationKind};
use ft_storage::{Storage as _, StorageFilter};
use serde::Serialize;

use crate::cli::{DocAddArgs, DocIndexArgs, DocLinkArgs, GlobalOpts};
use crate::commands::CommandOutcome;
use crate::context::{WorkCtx, append_relation};
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

    let (parsed_title, summary) = parse_doc_meta(&content);
    let title = args
        .title
        .clone()
        .or(parsed_title)
        .unwrap_or_else(|| file_stem(&rel_path));
    let created_by = ctx.actor()?;

    let mut builder = RecordBuilder::new(RecordKind::Doc, &title, created_by).doc(Doc {
        path: rel_path.clone(),
        content_hash: ft_embed::content_hash(&content),
        title: title.clone(),
        summary,
        doc_type: args.doc_type.clone(),
        trust: ft_core::TrustState::Draft,
    });
    if let Some(scope) = &args.scope {
        builder = builder.owning_scope(scope);
    }
    let mut record = builder
        .build()
        .map_err(|e| CliError::internal(ADD, format!("build doc: {e}")))?;
    ctx.save_record(&mut record)?;

    let id = record.envelope.id.as_str().to_string();
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
    for id in targets {
        let record = ctx.read_record(&id)?;
        let RecordBody::Doc(doc) = &record.body else {
            warnings.push(format!("{id} is not a doc record — skipped"));
            continue;
        };
        match std::fs::read_to_string(root.join(&doc.path)) {
            Ok(content) => {
                let new_hash = ft_embed::content_hash(&content);
                if new_hash == doc.content_hash {
                    continue; // already fresh
                }
                let (_t, summary) = parse_doc_meta(&content);
                let mut updated = record.clone();
                if let RecordBody::Doc(d) = &mut updated.body {
                    d.content_hash = new_hash;
                    d.summary = summary;
                }
                ctx.save_record(&mut updated)?;
                refreshed.push(id.as_str().to_string());
            }
            Err(_) => warnings.push(format!(
                "doc {id} points at a missing file ({}) — broken link",
                doc.path
            )),
        }
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

/// Extract `(title, summary)` from markdown: skips YAML frontmatter, takes the
/// first `# H1` as the title and the first prose paragraph as the summary
/// (capped so the record stays a thin pointer).
fn parse_doc_meta(text: &str) -> (Option<String>, String) {
    let body = strip_frontmatter(text);
    let mut title = None;
    let mut summary = String::new();
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(h1) = line.strip_prefix("# ") {
            if title.is_none() {
                title = Some(h1.trim().to_string());
            }
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        summary = line.to_string();
        break;
    }
    if summary.len() > 280 {
        summary.truncate(277);
        summary.push_str("...");
    }
    (title, summary)
}

/// Drop a leading `---\n … \n---` YAML frontmatter block if present.
fn strip_frontmatter(text: &str) -> &str {
    let t = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"));
    if let Some(rest) = t {
        if let Some(end) = rest.find("\n---") {
            let after = &rest[end + 4..];
            return after.trim_start_matches(['\r', '\n']);
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::{parse_doc_meta, strip_frontmatter};

    #[test]
    fn parses_title_and_summary_skipping_frontmatter_and_heading() {
        let md = "---\ndoc_type: design\nlinks:\n  - x\n---\n# The Title\n\nThe first prose paragraph.\nmore.\n";
        let (title, summary) = parse_doc_meta(md);
        assert_eq!(title.as_deref(), Some("The Title"));
        assert_eq!(summary, "The first prose paragraph.");
    }

    #[test]
    fn no_frontmatter_no_heading() {
        let (title, summary) = parse_doc_meta("just a body line\nsecond");
        assert_eq!(title, None);
        assert_eq!(summary, "just a body line");
    }

    #[test]
    fn strip_frontmatter_leaves_body_when_absent() {
        assert_eq!(strip_frontmatter("# H\nbody"), "# H\nbody");
    }

    #[test]
    fn summary_is_capped() {
        let long = format!("# T\n\n{}", "x".repeat(400));
        let (_t, summary) = parse_doc_meta(&long);
        assert!(summary.len() <= 280);
        assert!(summary.ends_with("..."));
    }
}

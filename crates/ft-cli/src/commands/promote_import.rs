//! `firetrail promote-import …` — list / promote quarantined imports (M6).
//!
//! Without an id, lists every quarantined record that meets the inbound-ref
//! threshold (per [`ft_import::PromotionOpts`]). With an id, promotes that one
//! record. `--batch` promotes every candidate non-interactively.
//!
//! Promotion clears the `quarantine=true` label and records an audit history
//! entry (ADR-0017) — see [`ft_import::promote_record`].

use ft_import::{PromotionOpts, is_quarantined, promote_record, promotion_candidates};
use serde::Serialize;

use crate::cli::{GlobalOpts, PromoteImportArgs};
use crate::commands::CommandOutcome;
use crate::context::WorkCtx;
use crate::error::CliError;

const COMMAND: &str = "promote-import";

/// `firetrail promote-import [id]`
#[allow(clippy::too_many_lines)]
pub fn run(args: &PromoteImportArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let mut ctx = WorkCtx::open(COMMAND, global.workspace.as_deref())?;
    let warnings = ctx.warnings.clone();

    let opts = PromotionOpts {
        min_inbound_refs: args
            .min_inbound_refs
            .unwrap_or(PromotionOpts::default().min_inbound_refs),
    };

    if let Some(raw) = &args.id {
        // Targeted promotion. Verify the record exists, is quarantined, and
        // meets the inbound-refs threshold (unless `--force` overrides).
        let id = ctx.resolve_id(raw)?;
        let record = ctx.read_record(&id)?;
        if !is_quarantined(&record) {
            return Err(CliError::UserError {
                command: COMMAND.to_string(),
                message: format!("record `{}` is not quarantined", id.as_str()),
                details: serde_json::json!({ "id": id.as_str() }),
            });
        }
        if !args.force {
            let cands = promotion_candidates(&ctx.storage, &opts)
                .map_err(|e| CliError::internal(COMMAND, format!("scan candidates: {e}")))?;
            if !cands.iter().any(|c| c.id == id) {
                return Err(CliError::UserError {
                    command: COMMAND.to_string(),
                    message: format!(
                        "record `{}` does not meet the inbound-ref threshold ({}); pass `--force` to override",
                        id.as_str(),
                        opts.min_inbound_refs,
                    ),
                    details: serde_json::json!({
                        "id": id.as_str(),
                        "min_inbound_refs": opts.min_inbound_refs,
                    }),
                });
            }
        }
        promote_record(&ctx.storage, &id)
            .map_err(|e| CliError::internal(COMMAND, format!("promote: {e}")))?;

        // Refresh the index so the (now-canonical) record's labels reflect in
        // search and the next `promote-import` listing.
        refresh_after_promote(&mut ctx)?;

        return Ok(CommandOutcome::PromoteImport(PromoteImportOutcome {
            command: COMMAND,
            action: "promote",
            promoted_ids: vec![id.as_str().to_string()],
            candidates: Vec::new(),
            min_inbound_refs: opts.min_inbound_refs,
            warnings,
        }));
    }

    let candidates = promotion_candidates(&ctx.storage, &opts)
        .map_err(|e| CliError::internal(COMMAND, format!("scan candidates: {e}")))?;

    if args.batch {
        let mut promoted = Vec::with_capacity(candidates.len());
        for cand in &candidates {
            promote_record(&ctx.storage, &cand.id)
                .map_err(|e| CliError::internal(COMMAND, format!("promote: {e}")))?;
            promoted.push(cand.id.as_str().to_string());
        }
        if !promoted.is_empty() {
            refresh_after_promote(&mut ctx)?;
        }
        return Ok(CommandOutcome::PromoteImport(PromoteImportOutcome {
            command: COMMAND,
            action: "batch",
            promoted_ids: promoted,
            candidates: candidates.iter().map(CandidateView::from).collect(),
            min_inbound_refs: opts.min_inbound_refs,
            warnings,
        }));
    }

    if args.interactive {
        use crate::prompt::{PromptChoice, ask, is_interactive};

        let mut out_warnings = warnings;
        if !is_interactive() {
            out_warnings.push(
                "stdin is not a TTY; --interactive falls back to a non-mutating list".to_string(),
            );
            return Ok(CommandOutcome::PromoteImport(PromoteImportOutcome {
                command: COMMAND,
                action: "list",
                promoted_ids: Vec::new(),
                candidates: candidates.iter().map(CandidateView::from).collect(),
                min_inbound_refs: opts.min_inbound_refs,
                warnings: out_warnings,
            }));
        }

        let mut promoted = Vec::with_capacity(candidates.len());
        let mut aborted = false;
        for cand in &candidates {
            let q = format!(
                "promote `{}` ({} inbound refs)? [y/N/q]",
                cand.id.as_str(),
                cand.inbound_refs
            );
            let choice = ask(&q, PromptChoice::No)
                .map_err(|e| CliError::internal(COMMAND, format!("prompt: {e}")))?;
            match choice {
                PromptChoice::Yes => {
                    promote_record(&ctx.storage, &cand.id)
                        .map_err(|e| CliError::internal(COMMAND, format!("promote: {e}")))?;
                    promoted.push(cand.id.as_str().to_string());
                }
                PromptChoice::No => {}
                PromptChoice::Quit => {
                    aborted = true;
                    break;
                }
            }
        }
        if !promoted.is_empty() {
            refresh_after_promote(&mut ctx)?;
        }
        if aborted {
            out_warnings.push("interactive session aborted by user".to_string());
        }
        return Ok(CommandOutcome::PromoteImport(PromoteImportOutcome {
            command: COMMAND,
            action: "interactive",
            promoted_ids: promoted,
            candidates: candidates.iter().map(CandidateView::from).collect(),
            min_inbound_refs: opts.min_inbound_refs,
            warnings: out_warnings,
        }));
    }

    Ok(CommandOutcome::PromoteImport(PromoteImportOutcome {
        command: COMMAND,
        action: "list",
        promoted_ids: Vec::new(),
        candidates: candidates.iter().map(CandidateView::from).collect(),
        min_inbound_refs: opts.min_inbound_refs,
        warnings,
    }))
}

fn refresh_after_promote(ctx: &mut WorkCtx) -> Result<(), CliError> {
    use ft_search::SearchEngine;
    use ft_storage::{Storage as _, StorageFilter};

    let ids = ctx
        .storage
        .list(&StorageFilter::default())
        .map_err(|e| CliError::internal(COMMAND, format!("list storage: {e}")))?;
    let paths: Vec<_> = ids.iter().map(|id| ctx.storage.path_for(id)).collect();
    ctx.index
        .refresh(&ctx.storage, &paths, &[])
        .map_err(|e| CliError::internal(COMMAND, format!("refresh index: {e}")))?;
    let engine = SearchEngine::open(ctx.index.db_path())
        .map_err(|e| CliError::internal(COMMAND, format!("open search: {e}")))?;
    engine
        .ensure_schema()
        .map_err(|e| CliError::internal(COMMAND, format!("ensure search schema: {e}")))?;
    for id in &ids {
        let rec = ctx
            .storage
            .read(id)
            .map_err(|e| CliError::internal(COMMAND, format!("read: {e}")))?;
        engine
            .upsert_lexical(&rec)
            .map_err(|e| CliError::internal(COMMAND, format!("upsert search: {e}")))?;
    }
    Ok(())
}

/// JSON / markdown view for `firetrail promote-import`.
#[derive(Debug, Clone, Serialize)]
pub struct PromoteImportOutcome {
    /// Stable command name.
    #[serde(skip)]
    pub command: &'static str,
    /// Action label (list / promote / batch).
    pub action: &'static str,
    /// Records that were promoted (empty for listing).
    pub promoted_ids: Vec<String>,
    /// Candidates that meet the threshold.
    pub candidates: Vec<CandidateView>,
    /// Threshold used for this invocation.
    pub min_inbound_refs: usize,
    /// Non-fatal warnings.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<String>,
}

/// Serialised candidate.
#[derive(Debug, Clone, Serialize)]
pub struct CandidateView {
    /// Candidate id.
    pub id: String,
    /// Inbound reference count.
    pub inbound_refs: usize,
    /// Sample of referencing record ids.
    pub referencing_ids: Vec<String>,
}

impl From<&ft_import::PromotionCandidate> for CandidateView {
    fn from(c: &ft_import::PromotionCandidate) -> Self {
        Self {
            id: c.id.as_str().to_string(),
            inbound_refs: c.inbound_refs,
            referencing_ids: c
                .referencing_ids
                .iter()
                .map(|id| id.as_str().to_string())
                .collect(),
        }
    }
}

impl PromoteImportOutcome {
    /// Markdown rendering.
    pub fn markdown(&self) -> String {
        use std::fmt::Write as _;
        let mut s = format!(
            "**{}** action={} candidates={} promoted={} min_refs={}\n",
            self.command,
            self.action,
            self.candidates.len(),
            self.promoted_ids.len(),
            self.min_inbound_refs,
        );
        if !self.promoted_ids.is_empty() {
            s.push_str("\n## Promoted\n");
            for id in &self.promoted_ids {
                let _ = writeln!(s, "- `{id}`");
            }
        }
        if !self.candidates.is_empty() {
            s.push_str("\n## Candidates\n");
            for c in &self.candidates {
                let _ = writeln!(s, "- `{}` ({} inbound refs)", c.id, c.inbound_refs);
            }
        }
        s
    }
    /// Quiet line.
    pub fn quiet_line(&self) -> String {
        format!(
            "{}: action={} candidates={} promoted={}",
            self.command,
            self.action,
            self.candidates.len(),
            self.promoted_ids.len()
        )
    }
}

//! `firetrail prime` — produce a context pack for a task or query.

use std::collections::HashSet;

use chrono::Utc;
use ft_import::is_quarantined;
use ft_prime::{ContextPack, PrimeFormat, PrimeOptions, prime_for_query, prime_for_task};
use ft_storage::Storage as _;
use serde::Serialize;
use serde_json::Value;

use crate::cli::{FormatArg, GlobalOpts, PrimeArgs};
use crate::commands::CommandOutcome;
use crate::context::WorkCtx;
use crate::error::CliError;

const COMMAND: &str = "prime";

/// `firetrail prime`
pub fn run(args: &PrimeArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    if args.task.is_none() && args.query.is_none() {
        return Err(CliError::user(
            COMMAND,
            "must supply either --task <id> or --query <text>",
        ));
    }

    let mut ctx = WorkCtx::open(COMMAND, global.workspace.as_deref())?;

    // Map the (already-resolved) global format to the prime format: if the
    // user asked for JSON we tell ft-prime so its budget heuristics adapt.
    let format = match global.format {
        Some(FormatArg::Json) => PrimeFormat::Json,
        Some(FormatArg::Markdown) => PrimeFormat::Markdown,
        None => {
            if global.json {
                PrimeFormat::Json
            } else {
                PrimeFormat::Markdown
            }
        }
    };

    let mut opts = PrimeOptions {
        max_tokens: args.max_tokens,
        format,
        now: Utc::now(),
        ..PrimeOptions::default()
    };
    if let Some(t) = args.min_trust {
        opts.min_trust = Some(t.to_core());
    }
    if !args.kinds.is_empty() {
        opts.kind_filter = args.kinds.iter().map(|k| k.to_core()).collect();
    }
    if let Some(s) = &args.scope {
        opts.scope_filter = Some(s.clone());
    }

    let mut pack = if let Some(raw) = &args.task {
        let id = ctx.resolve_id(raw)?;
        // Lazy freshness backbone: warn on linked docs whose .md changed out of
        // band or went missing, so the pack never silently presents stale docs.
        refresh_stale_linked_docs(&mut ctx, &id);
        prime_for_task(&ctx.storage, &ctx.index, &id, &opts)
            .map_err(|e| CliError::internal(COMMAND, format!("prime: {e}")))?
    } else {
        let q = args
            .query
            .as_deref()
            .expect("checked above: one of --task / --query must be set");
        prime_for_query(&ctx.storage, &ctx.index, q, &opts)
            .map_err(|e| CliError::internal(COMMAND, format!("prime: {e}")))?
    };

    // Captured after the freshness pass so stale/broken linked-doc notes surface.
    let warnings = ctx.warnings.clone();

    // Quarantine filter (ADR-0014). The default ContextPack from ft-prime is
    // import-agnostic; we materialise the filter at the CLI layer (per
    // firetrail-2z2 design note) by inspecting each item's source record.
    // Re-tally `total_tokens` after the drop so downstream renderers see a
    // consistent budget view.
    let mut quarantined_ids: HashSet<String> = HashSet::new();
    if args.include_quarantine {
        for item in &pack.items {
            if let Ok(rec) = ctx.storage.read(&item.id)
                && is_quarantined(&rec)
            {
                quarantined_ids.insert(item.id.as_str().to_string());
            }
        }
    } else {
        pack.items.retain(|item| match ctx.storage.read(&item.id) {
            Ok(rec) => !is_quarantined(&rec),
            Err(_) => true,
        });
        pack.total_tokens = pack.items.iter().map(|i| i.tokens).sum();
    }

    let rendered = match opts.format {
        PrimeFormat::Markdown => Rendered::Markdown(ft_prime::render_markdown(&pack)),
        PrimeFormat::Json => Rendered::Json(ft_prime::render_json(&pack)),
    };

    Ok(CommandOutcome::Prime(PrimeOutcome {
        pack,
        rendered,
        warnings,
        quarantined_ids,
    }))
}

/// Lazy freshness backbone for file-backed docs (firetrail-2mwp.6).
///
/// Checks each `Doc` linked to the target and warns when its `.md` file changed
/// since indexing (stale) or is missing (broken link). Read-only on purpose:
/// prime already delivers the doc's `path`, so the agent reads the *live* file
/// and always sees current content. Rewriting the record's `content_hash` to
/// refresh search belongs to `firetrail doc index` / the git hook, where there
/// is an explicit actor — doing it here would pollute the audit chain on every
/// prime. Best-effort: index/read failures are silently skipped.
fn refresh_stale_linked_docs(ctx: &mut WorkCtx, target: &ft_core::RecordId) {
    let Ok(edges) = ctx.index.relations(target) else {
        return;
    };
    let root = ctx.ws.root.clone();
    let mut seen = HashSet::new();
    let mut notes = Vec::new();
    for edge in edges {
        let other = if &edge.from == target {
            edge.to
        } else {
            edge.from
        };
        if &other == target || !seen.insert(other.clone()) {
            continue;
        }
        let Ok(rec) = ctx.read_record(&other) else {
            continue;
        };
        if let ft_core::RecordBody::Doc(doc) = &rec.body {
            match ft_embed::doc_freshness(&root, doc) {
                ft_embed::DocFreshness::Stale => notes.push(format!(
                    "linked doc {} ({}) changed since indexing — prime delivers the live file path; run `firetrail doc index` to refresh search",
                    other.as_str(),
                    doc.path
                )),
                ft_embed::DocFreshness::Missing => notes.push(format!(
                    "linked doc {} points at a missing file ({}) — broken link",
                    other.as_str(),
                    doc.path
                )),
                ft_embed::DocFreshness::Fresh => {}
            }
        }
    }
    ctx.warnings.extend(notes);
}

#[derive(Debug, Clone)]
enum Rendered {
    Markdown(String),
    Json(Value),
}

/// Outcome of `firetrail prime`.
#[derive(Debug, Clone)]
pub struct PrimeOutcome {
    /// The selected context pack.
    pub pack: ContextPack,
    /// Pre-rendered representation, format chosen by the caller.
    rendered: Rendered,
    /// Non-fatal warnings.
    pub warnings: Vec<String>,
    /// Ids of items in the pack that are quarantined imports. Empty when the
    /// caller did not pass `--include-quarantine` (those are filtered out).
    quarantined_ids: HashSet<String>,
}

impl PrimeOutcome {
    /// Markdown rendering — uses `ft_prime::render_markdown` when the caller
    /// asked for markdown, otherwise serialises the JSON view inline.
    pub fn markdown(&self) -> String {
        match &self.rendered {
            Rendered::Markdown(s) => s.clone(),
            Rendered::Json(v) => {
                serde_json::to_string_pretty(v).unwrap_or_else(|_| "<render error>".into())
            }
        }
    }
    /// One-line quiet summary.
    pub fn quiet_line(&self) -> String {
        format!(
            "prime: {} items, {}/{} tokens",
            self.pack.items.len(),
            self.pack.total_tokens,
            self.pack.budget,
        )
    }
    /// JSON payload for the success envelope.
    pub fn json_data(&self) -> Value {
        // Always emit the structured pack (plus rendered markdown when that
        // format was requested) so consumers can pick whichever they want.
        let mut data = ft_prime::render_json(&self.pack);
        if let Rendered::Markdown(s) = &self.rendered {
            if let Some(obj) = data.as_object_mut() {
                obj.insert("rendered_markdown".to_string(), Value::String(s.clone()));
            }
        }
        // Stamp `quarantine: true` on items whose source record is
        // quarantined. ADR-0014 says agents should know when they're reading
        // an unreviewed import even with `--include-quarantine`.
        if !self.quarantined_ids.is_empty()
            && let Some(items) = data.get_mut("items").and_then(Value::as_array_mut)
        {
            for item in items {
                if let Some(obj) = item.as_object_mut()
                    && let Some(id) = obj.get("id").and_then(Value::as_str)
                    && self.quarantined_ids.contains(id)
                {
                    obj.insert("quarantine".to_string(), Value::Bool(true));
                }
            }
        }
        data
    }
}

// Tiny shim so the envelope serialiser doesn't need to know about
// `PrimeOutcome` internals.
impl Serialize for PrimeOutcome {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        self.json_data().serialize(ser)
    }
}

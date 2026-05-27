//! `firetrail prime` — produce a context pack for a task or query.

use chrono::Utc;
use ft_prime::{ContextPack, PrimeFormat, PrimeOptions, prime_for_query, prime_for_task};
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

    let ctx = WorkCtx::open(COMMAND, global.workspace.as_deref())?;
    let warnings = ctx.warnings.clone();

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

    let pack = if let Some(raw) = &args.task {
        let id = ctx.resolve_id(raw)?;
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

    let rendered = match opts.format {
        PrimeFormat::Markdown => Rendered::Markdown(ft_prime::render_markdown(&pack)),
        PrimeFormat::Json => Rendered::Json(ft_prime::render_json(&pack)),
    };

    Ok(CommandOutcome::Prime(PrimeOutcome {
        pack,
        rendered,
        warnings,
    }))
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

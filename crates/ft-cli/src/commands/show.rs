//! `firetrail show <id>` — full record + relations.

use ft_core::{Record, Relation};
use serde::Serialize;

use crate::cli::{GlobalOpts, ShowArgs};
use crate::commands::CommandOutcome;
use crate::context::{WorkCtx, load_relations};
use crate::error::CliError;

const COMMAND: &str = "show";

/// Entry point.
pub fn run(args: &ShowArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let ctx = WorkCtx::open(COMMAND, global.workspace.as_deref())?;
    let warnings = ctx.warnings.clone();
    let id = ctx.resolve_id(&args.id)?;
    let record = ctx.read_record(&id)?;

    let all = load_relations(&ctx.ws)?;
    let relations: Vec<Relation> = all
        .into_iter()
        .filter(|r| r.from == id || r.to == id)
        .collect();

    Ok(CommandOutcome::Show(ShowOutcome {
        record,
        relations,
        warnings,
    }))
}

#[derive(Debug, Clone, Serialize)]
pub struct ShowOutcome {
    pub record: Record,
    pub relations: Vec<Relation>,
    /// Non-fatal warnings (e.g. index auto-rebuild on open).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<String>,
}

impl ShowOutcome {
    pub fn markdown(&self) -> String {
        use std::fmt::Write as _;
        let env = &self.record.envelope;
        let mut s = format!(
            "# {kind:?} `{id}`\n\n**{title}**\n\nstatus: {status:?} · priority: {priority:?}\nowner: {owner}\ncreated_by: {created_by} · created_at: {created_at}\nupdated_at: {updated_at}\n",
            kind = env.kind,
            id = env.id,
            title = env.title,
            status = env.status,
            priority = env.priority,
            owner = env
                .owner
                .as_ref()
                .map_or_else(|| "—".to_string(), |o| o.as_str().to_string()),
            created_by = env.created_by,
            created_at = env.created_at.to_rfc3339(),
            updated_at = env.updated_at.to_rfc3339(),
        );

        if let Some(scope) = &env.owning_scope {
            let _ = writeln!(s, "scope: {scope}");
        }
        if !env.labels.is_empty() {
            s.push_str("\n## Labels\n");
            for l in &env.labels {
                let _ = writeln!(s, "- `{}={}`", l.key, l.value);
            }
        }

        if !self.relations.is_empty() {
            s.push_str("\n## Relations\n");
            for r in &self.relations {
                let kind = serde_json::to_value(r.kind)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_else(|| format!("{:?}", r.kind));
                let _ = writeln!(s, "- `{}` --{}--> `{}`", r.from, kind, r.to);
            }
        }

        s.push_str("\n## State hash\n");
        let _ = writeln!(s, "`{}`", env.state_hash);
        s
    }

    pub fn quiet_line(&self) -> String {
        format!(
            "{}: {}",
            self.record.envelope.id, self.record.envelope.title
        )
    }
}

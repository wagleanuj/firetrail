//! `firetrail board` — kanban-style snapshot grouped by status.

use ft_core::Identity;
use ft_index::{IndexedRecord, ListQuery};
use serde::Serialize;

use crate::cli::{BoardArgs, GlobalOpts};
use crate::commands::CommandOutcome;
use crate::context::WorkCtx;
use crate::error::CliError;

const COMMAND: &str = "board";

/// Entry point.
pub fn run(args: &BoardArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let ctx = WorkCtx::open(COMMAND, global.workspace.as_deref())?;
    let warnings = ctx.warnings.clone();
    let mut q = ListQuery {
        include_closed: true,
        include_archived: false,
        ..ListQuery::default()
    };
    if let Some(o) = &args.owner {
        let identity = Identity::new(o.clone())
            .map_err(|e| CliError::user(COMMAND, format!("invalid owner: {e}")))?;
        q.owners = Some(vec![identity]);
    }
    if let Some(s) = &args.scope {
        q.scopes = Some(vec![s.clone()]);
    }
    let rows = ctx
        .index
        .list(&q)
        .map_err(|e| CliError::internal(COMMAND, e))?;
    let mut outcome = build_board(&rows);
    outcome.warnings = warnings;
    Ok(CommandOutcome::Board(outcome))
}

fn build_board(rows: &[IndexedRecord]) -> BoardOutcome {
    use ft_core::Status;
    let mut todo = Vec::new();
    let mut in_progress = Vec::new();
    let mut review = Vec::new();
    let mut done = Vec::new();
    for r in rows {
        let card = BoardCard {
            id: r.id.as_str().to_string(),
            short_id: r.id.short(8).to_string(),
            title: r.title.clone(),
            priority: format!("{:?}", r.priority).to_lowercase(),
            owner: r.owner.as_ref().map(|o| o.as_str().to_string()),
        };
        match r.status {
            Status::Open | Status::Ready => todo.push(card),
            Status::InProgress | Status::Blocked => in_progress.push(card),
            Status::Review => review.push(card),
            Status::Closed => done.push(card),
            _ => {}
        }
    }
    // Stable order: by id alphabetic so snapshot tests are deterministic.
    for col in [&mut todo, &mut in_progress, &mut review, &mut done] {
        col.sort_by(|a, b| a.id.cmp(&b.id));
    }
    BoardOutcome {
        todo,
        in_progress,
        review,
        done,
        warnings: Vec::new(),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct BoardCard {
    pub id: String,
    pub short_id: String,
    pub title: String,
    pub priority: String,
    pub owner: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BoardOutcome {
    pub todo: Vec<BoardCard>,
    pub in_progress: Vec<BoardCard>,
    pub review: Vec<BoardCard>,
    pub done: Vec<BoardCard>,
    /// Non-fatal warnings (e.g. index auto-rebuild on open).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<String>,
}

impl BoardOutcome {
    pub fn markdown(&self) -> String {
        const COL: usize = 22;
        let cols = [
            ("TODO", &self.todo),
            ("IN PROGRESS", &self.in_progress),
            ("REVIEW", &self.review),
            ("DONE", &self.done),
        ];
        let max_rows = cols.iter().map(|(_, c)| c.len()).max().unwrap_or(0);
        let mut s = String::new();
        // Header
        for (h, _) in &cols {
            s.push_str(&pad(h, COL));
            s.push(' ');
        }
        s.push('\n');
        s.push_str(&"─".repeat((COL + 1) * cols.len() - 1));
        s.push('\n');
        for row in 0..max_rows {
            // Three lines per card: short_id, title, priority+owner
            for line in 0..3 {
                for (_, col) in &cols {
                    let text = col.get(row).map_or(String::new(), |c| match line {
                        0 => c.short_id.clone(),
                        1 => truncate(&c.title, COL),
                        _ => format!(
                            "{} {}",
                            c.priority,
                            c.owner
                                .as_deref()
                                .map(|o| format!("@{o}"))
                                .unwrap_or_default()
                        ),
                    });
                    s.push_str(&pad(&text, COL));
                    s.push(' ');
                }
                s.push('\n');
            }
            s.push('\n');
        }
        s
    }

    pub fn quiet_line(&self) -> String {
        format!(
            "board: {} todo, {} in_progress, {} review, {} done",
            self.todo.len(),
            self.in_progress.len(),
            self.review.len(),
            self.done.len()
        )
    }
}

fn pad(s: &str, width: usize) -> String {
    let truncated = truncate(s, width);
    let mut out = truncated.clone();
    let len = truncated.chars().count();
    if len < width {
        out.push_str(&" ".repeat(width - len));
    }
    out
}

fn truncate(s: &str, width: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= width {
        s.to_string()
    } else {
        let mut out: String = chars[..width.saturating_sub(1)].iter().collect();
        out.push('…');
        out
    }
}

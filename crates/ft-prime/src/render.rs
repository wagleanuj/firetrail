//! Output formatters.

use std::fmt::Write as _;

use serde_json::{Value, json};

use crate::pack::{ContextPack, OmittedReason};

/// Render `pack` as ADR-0019-style markdown.
#[must_use]
pub fn render_markdown(pack: &ContextPack) -> String {
    let mut out = String::new();
    out.push_str("# Firetrail context pack\n\n");
    if let Some(id) = &pack.target_id {
        let _ = writeln!(out, "Target: `{id}`\n");
    }
    if let Some(q) = &pack.query {
        let _ = writeln!(out, "Query: `{q}`\n");
    }
    let _ = writeln!(
        out,
        "Budget: {} tokens · Used: {} tokens · Items: {}\n",
        pack.budget,
        pack.total_tokens,
        pack.items.len()
    );

    if pack.items.is_empty() {
        out.push_str("_No matching records._\n\n");
    } else {
        out.push_str("## Records\n\n");
        for item in &pack.items {
            let _ = writeln!(
                out,
                "### {} `{}` [{:?}, score {:.3}, {} tok]",
                item.title, item.id, item.trust, item.score, item.tokens
            );
            out.push('\n');
            if item.body_excerpt.trim().is_empty() {
                out.push_str("_(no body)_\n\n");
            } else {
                out.push_str(item.body_excerpt.trim_end());
                out.push_str("\n\n");
            }
        }
    }

    if !pack.omitted.is_empty() {
        out.push_str("## Omitted from this context pack\n\n");
        out.push_str(
            "The following records matched but were not included. Run with a larger\n\
             `--max-tokens`, relax filters, or fetch the listed IDs directly.\n\n",
        );
        for e in &pack.omitted {
            let reason = match e.reason {
                OmittedReason::BudgetExceeded => "budget",
                OmittedReason::TooStale => "stale",
                OmittedReason::BelowTrustFloor => "below trust floor",
                OmittedReason::ScopeFiltered => "filtered",
            };
            let _ = writeln!(out, "- `{}` [{:?}] — {reason}", e.id, e.kind);
        }
        out.push('\n');
        let _ = writeln!(out, "Total omitted: {} records.\n", pack.omitted.len());
    }

    out
}

/// Render `pack` as JSON.
#[must_use]
pub fn render_json(pack: &ContextPack) -> Value {
    serde_json::to_value(pack).unwrap_or_else(|_| json!({"error": "serialization failed"}))
}

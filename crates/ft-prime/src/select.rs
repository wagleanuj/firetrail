//! Selection, ranking, and budget packing.

use std::collections::HashSet;

use ft_core::{Record, RecordBody, RecordId, RecordKind, TrustState};
use ft_index::Index;
use ft_storage::{Storage, StorageFilter};

use crate::error::PrimeError;
use crate::estimate::estimate_tokens;
use crate::options::PrimeOptions;
use crate::pack::{ContextPack, OmittedEntry, OmittedReason, PackItem};
use crate::score::{compose, meets_trust_floor, recency_factor};

/// Build a [`ContextPack`] for a specific task / record id.
///
/// Walks the record itself, its parent epic, its blockers (via the index),
/// and other records that share its scope or affected paths. Results are
/// ranked and packed greedily against `opts.max_tokens`.
///
/// # Errors
///
/// Returns [`PrimeError::TargetNotFound`] if `task_id` cannot be read,
/// [`PrimeError::Storage`] / [`PrimeError::Index`] for backing failures.
pub fn prime_for_task(
    storage: &dyn Storage,
    index: &Index,
    task_id: &RecordId,
    opts: &PrimeOptions,
) -> Result<ContextPack, PrimeError> {
    let target = storage
        .read(task_id)
        .map_err(|_| PrimeError::TargetNotFound(task_id.as_str().to_string()))?;

    let mut candidates: Vec<Candidate> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    push_candidate(
        &mut candidates,
        &mut seen,
        &target,
        Provenance::Target,
        opts,
    );

    // Structural relations from the index (blocked-by / blocks / parent-of /
    // child-of / related-to). Direct edges only — ADR-0019 priority level 3.
    let edges = index.relations(task_id)?;
    for edge in edges {
        let other = if edge.from == *task_id {
            edge.to
        } else {
            edge.from
        };
        if let Ok(rec) = storage.read(&other) {
            push_candidate(
                &mut candidates,
                &mut seen,
                &rec,
                Provenance::StructuralRelation,
                opts,
            );
        }
    }

    // Same-scope records: cheap fallback search via storage list.
    if let Some(scope) = target.envelope.owning_scope.as_ref() {
        let filter = StorageFilter::default();
        for row in storage.iter(&filter) {
            let Ok(rec) = row else { continue };
            if rec.envelope.id == *task_id {
                continue;
            }
            if rec.envelope.owning_scope.as_deref() == Some(scope.as_str()) {
                push_candidate(
                    &mut candidates,
                    &mut seen,
                    &rec,
                    Provenance::SameScope,
                    opts,
                );
            }
        }
    }

    Ok(pack(candidates, Some(task_id.clone()), None, opts))
}

/// Build a [`ContextPack`] from a free-form keyword query.
///
/// This is a substring / word match — **not** semantic search. The
/// `ft-search` crate owns vector similarity; `ft-prime` intentionally avoids
/// pulling in embedding dependencies.
///
/// # Errors
///
/// Returns [`PrimeError::EmptyQuery`] if `query` trims to empty. Propagates
/// storage / index failures.
pub fn prime_for_query(
    storage: &dyn Storage,
    _index: &Index,
    query: &str,
    opts: &PrimeOptions,
) -> Result<ContextPack, PrimeError> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Err(PrimeError::EmptyQuery);
    }

    let terms = tokenize_query(trimmed);
    let mut candidates: Vec<Candidate> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    let filter = StorageFilter::default();
    for row in storage.iter(&filter) {
        let Ok(rec) = row else { continue };
        if !record_matches_terms(&rec, &terms) {
            continue;
        }
        push_candidate(&mut candidates, &mut seen, &rec, Provenance::QueryHit, opts);
    }

    Ok(pack(candidates, None, Some(trimmed.to_string()), opts))
}

// ─── internals ──────────────────────────────────────────────────────────────

/// How a candidate entered the selection set. Used to derive a relevance score
/// and to honour ADR-0019's "never truncate the target / direct relations"
/// rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Provenance {
    Target,
    StructuralRelation,
    SameScope,
    QueryHit,
}

impl Provenance {
    fn relevance(self) -> f32 {
        match self {
            Self::Target | Self::StructuralRelation => 1.0,
            Self::QueryHit => 0.5,
            Self::SameScope => 0.3,
        }
    }

    fn is_required(self) -> bool {
        matches!(self, Self::Target | Self::StructuralRelation)
    }

    /// Coarse rank used as the primary sort key so the target is always
    /// first, followed by direct structural relations, then everything else.
    fn rank(self) -> u8 {
        match self {
            Self::Target => 0,
            Self::StructuralRelation => 1,
            Self::QueryHit => 2,
            Self::SameScope => 3,
        }
    }
}

#[derive(Debug, Clone)]
struct Candidate {
    record: Record,
    provenance: Provenance,
    trust: TrustState,
    score: f32,
}

fn push_candidate(
    out: &mut Vec<Candidate>,
    seen: &mut HashSet<String>,
    record: &Record,
    provenance: Provenance,
    opts: &PrimeOptions,
) {
    let key = record.envelope.id.as_str().to_string();
    if !seen.insert(key) {
        return;
    }
    let trust = record_trust(record);
    let recency = recency_factor(record.envelope.updated_at, opts.now);
    let score = compose(trust, provenance.relevance(), recency);
    out.push(Candidate {
        record: record.clone(),
        provenance,
        trust,
        score,
    });
}

/// Greedy packing pass. `candidates` are sorted by `(provenance_required,
/// score, id)` and pulled in order until the budget is exhausted.
fn pack(
    mut candidates: Vec<Candidate>,
    target_id: Option<RecordId>,
    query: Option<String>,
    opts: &PrimeOptions,
) -> ContextPack {
    // Deterministic ordering: required items first, then by score desc, then
    // by id asc for tie-breaks.
    candidates.sort_by(|a, b| {
        a.provenance
            .rank()
            .cmp(&b.provenance.rank())
            .then(
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
            .then_with(|| {
                a.record
                    .envelope
                    .id
                    .as_str()
                    .cmp(b.record.envelope.id.as_str())
            })
    });

    let mut items: Vec<PackItem> = Vec::new();
    let mut omitted: Vec<OmittedEntry> = Vec::new();
    let mut used: usize = 0;
    let budget = opts.max_tokens;

    for c in candidates {
        // Apply filters first; record an OmittedEntry when a candidate is
        // dropped purely for filtering reasons.
        if !opts.kind_filter.is_empty() && !opts.kind_filter.contains(&c.record.envelope.kind) {
            omitted.push(OmittedEntry {
                id: c.record.envelope.id.clone(),
                kind: c.record.envelope.kind,
                reason: OmittedReason::ScopeFiltered,
            });
            continue;
        }
        if let Some(scope) = opts.scope_filter.as_ref() {
            if c.record.envelope.owning_scope.as_deref() != Some(scope.as_str()) {
                omitted.push(OmittedEntry {
                    id: c.record.envelope.id.clone(),
                    kind: c.record.envelope.kind,
                    reason: OmittedReason::ScopeFiltered,
                });
                continue;
            }
        }
        if !meets_trust_floor(c.trust, opts.min_trust) {
            omitted.push(OmittedEntry {
                id: c.record.envelope.id.clone(),
                kind: c.record.envelope.kind,
                reason: OmittedReason::BelowTrustFloor,
            });
            continue;
        }

        let (body, body_tokens, item_tokens) = excerpt_for(&c.record, budget);
        let item = PackItem {
            id: c.record.envelope.id.clone(),
            kind: c.record.envelope.kind,
            title: c.record.envelope.title.clone(),
            trust: c.trust,
            score: c.score,
            tokens: item_tokens,
            body_excerpt: body,
        };
        // Suppress unused-variable warning while keeping the local clear.
        let _ = body_tokens;

        if used.saturating_add(item.tokens) > budget && !c.provenance.is_required() {
            omitted.push(OmittedEntry {
                id: item.id,
                kind: item.kind,
                reason: OmittedReason::BudgetExceeded,
            });
            continue;
        }

        used = used.saturating_add(item.tokens);
        items.push(item);
    }

    ContextPack {
        target_id,
        query,
        items,
        total_tokens: used,
        budget,
        omitted,
    }
}

/// Truncated body excerpt and its token cost.
///
/// Returns `(excerpt, body_tokens, item_tokens)`. `item_tokens` accounts for
/// the title plus the body excerpt. If the full body would exceed 25% of the
/// budget, it is truncated to the first paragraph and the marker
/// `"...truncated..."` is appended.
fn excerpt_for(record: &Record, budget: usize) -> (String, usize, usize) {
    let title_tokens = estimate_tokens(&record.envelope.title);
    let full_body = body_text(record);
    let full_tokens = estimate_tokens(&full_body);
    let cap = budget / 4;

    if full_tokens <= cap || cap == 0 {
        return (full_body.clone(), full_tokens, title_tokens + full_tokens);
    }

    // Take the first non-empty paragraph, then append a truncation marker.
    let first_para = full_body
        .split("\n\n")
        .find(|p| !p.trim().is_empty())
        .unwrap_or("")
        .to_string();
    let mut truncated = first_para;
    truncated.push_str("\n\n...truncated...");
    let t_tokens = estimate_tokens(&truncated);
    (truncated, t_tokens, title_tokens + t_tokens)
}

/// Extract the "body" text of a record for excerpting.
fn body_text(record: &Record) -> String {
    match &record.body {
        RecordBody::Epic(e) => e.description.clone(),
        RecordBody::Task(t) => t.description.clone(),
        RecordBody::Subtask(s) => s.description.clone(),
        RecordBody::Bug(b) => b.description.clone(),
        RecordBody::Incident(i) => i.summary.clone(),
        RecordBody::Finding(f) => {
            if f.details.is_empty() {
                f.summary.clone()
            } else {
                format!("{}\n\n{}", f.summary, f.details)
            }
        }
        RecordBody::Runbook(r) => r.summary.clone(),
        RecordBody::Decision(d) => {
            if d.context.is_empty() {
                d.decision.clone()
            } else {
                format!("{}\n\n{}", d.context, d.decision)
            }
        }
        RecordBody::Gotcha(g) => {
            if g.details.is_empty() {
                g.summary.clone()
            } else {
                format!("{}\n\n{}", g.summary, g.details)
            }
        }
        RecordBody::Memory(m) => m.body.clone(),
        // TODO(firetrail-2mwp.6): deliver summary + path and let the agent read
        // the full file on demand (protects the token budget). Interim: the
        // stored summary excerpt.
        RecordBody::Doc(d) => d.summary.clone(),
    }
}

/// Trust state to record on the pack item. Non-memory work-tracking records
/// (Epic/Task/Subtask/Bug) do not carry trust; we treat them as `Verified`
/// for scoring purposes since they are authoritative work items.
pub(crate) fn record_trust(record: &Record) -> TrustState {
    match &record.body {
        RecordBody::Incident(i) => i.trust,
        RecordBody::Finding(f) => f.trust,
        RecordBody::Runbook(r) => r.trust,
        RecordBody::Decision(d) => d.trust,
        RecordBody::Gotcha(g) => g.trust,
        RecordBody::Memory(m) => m.trust,
        RecordBody::Doc(d) => d.trust,
        RecordBody::Epic(_) | RecordBody::Task(_) | RecordBody::Subtask(_) | RecordBody::Bug(_) => {
            TrustState::Verified
        }
    }
}

fn tokenize_query(q: &str) -> Vec<String> {
    q.split_whitespace()
        .map(|w| {
            w.trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
        })
        .filter(|w| !w.is_empty())
        .collect()
}

fn record_matches_terms(record: &Record, terms: &[String]) -> bool {
    if terms.is_empty() {
        return false;
    }
    let mut hay = record.envelope.title.to_lowercase();
    hay.push('\n');
    hay.push_str(&body_text(record).to_lowercase());
    terms.iter().any(|t| hay.contains(t))
}

// Keep `RecordKind` in scope so future expansions of kind-specific handling
// don't trigger unused-import warnings.
#[allow(dead_code)]
const _KIND_NOTE: Option<RecordKind> = None;

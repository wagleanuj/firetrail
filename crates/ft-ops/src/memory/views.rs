//! Read-only memory views: `list`, `stale`, `show`.
//!
//! Walks storage directly (rather than the index) because trust / risk
//! filtering needs to inspect the body, and the index does not surface
//! those fields at M2. Mirrors `ft_cli::commands::memory_views`.

use chrono::Utc;
use ft_core::{Record, RecordBody, RecordKind, RiskClass, TrustState};
use ft_storage::{Storage as _, StorageFilter};
use ft_trust::{StalePolicy, is_stale};
use serde::{Deserialize, Serialize};

use crate::error::OpsError;
use crate::events::EventBus;
use crate::identity::Identity;
use crate::workspace::Workspace;

use super::create::{MemoryKind, RiskClassInput};
use super::ctx::MemoryCtx;

/// All memory kinds (drives the default scan when `kind` is `None`).
const MEMORY_KINDS: &[RecordKind] = &[
    RecordKind::Incident,
    RecordKind::Finding,
    RecordKind::Runbook,
    RecordKind::Decision,
    RecordKind::Gotcha,
    RecordKind::Memory,
];

/// Trust-state filter on the wire. Mirrors `ft_core::TrustState`.
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TrustStateInput {
    /// Newly authored.
    Draft,
    /// Human-reviewed.
    Reviewed,
    /// Verified by two reviewers.
    Verified,
    /// Aged out.
    Stale,
    /// Deprecated.
    Deprecated,
    /// Archived.
    Archived,
    /// Superseded.
    Superseded,
    /// Rejected.
    Rejected,
    /// Redacted.
    Redacted,
}

impl TrustStateInput {
    fn to_core(self) -> TrustState {
        match self {
            Self::Draft => TrustState::Draft,
            Self::Reviewed => TrustState::Reviewed,
            Self::Verified => TrustState::Verified,
            Self::Stale => TrustState::Stale,
            Self::Deprecated => TrustState::Deprecated,
            Self::Archived => TrustState::Archived,
            Self::Superseded => TrustState::Superseded,
            Self::Rejected => TrustState::Rejected,
            Self::Redacted => TrustState::Redacted,
        }
    }
}

/// Input for [`list`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export, rename = "MemoryListInput"))]
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListInput {
    /// Restrict to one memory kind. `None` means all six.
    #[serde(default)]
    pub kind: Option<MemoryKind>,
    /// Filter by trust state.
    #[serde(default)]
    pub trust: Option<TrustStateInput>,
    /// Filter by risk class.
    #[serde(default)]
    pub risk_class: Option<RiskClassInput>,
    /// Only return records past their freshness window.
    #[serde(default)]
    pub stale: bool,
    /// Cap the number of rows.
    #[serde(default)]
    pub limit: Option<u64>,
}

/// One row in the list / stale response.
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryRowOut {
    /// Canonical record id.
    pub id: String,
    /// Memory kind (lowercase string).
    pub kind: String,
    /// Title / summary.
    pub title: String,
    /// Trust state.
    pub trust: Option<String>,
    /// Risk class.
    pub risk_class: Option<String>,
    /// Freshness flag.
    pub stale: bool,
}

/// Output of [`list`] / [`stale`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export, rename = "MemoryListOutput"))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListOutput {
    /// Matching rows.
    pub rows: Vec<MemoryRowOut>,
}

/// Input for [`stale`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export, rename = "MemoryStaleInput"))]
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StaleInput {
    /// Restrict to one memory kind.
    #[serde(default)]
    pub kind: Option<MemoryKind>,
}

/// Input for [`show`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export, rename = "MemoryShowInput"))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShowInput {
    /// Memory id (full canonical form or unambiguous prefix).
    pub id: String,
}

/// Output of [`show`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ShowOutput {
    /// The resolved record.
    pub record: Record,
}

/// `memory list` op.
#[allow(clippy::needless_pass_by_value)]
pub fn list(
    ws: &Workspace,
    identity: &Identity,
    input: ListInput,
    _events: &EventBus,
) -> Result<ListOutput, OpsError> {
    let ctx = MemoryCtx::open(ws, identity, "memory list")?;
    let rows = scan(
        &ctx,
        input.kind,
        input.trust,
        input.risk_class,
        input.stale,
        input.limit,
    )?;
    Ok(ListOutput { rows })
}

/// `memory stale` op.
#[allow(clippy::needless_pass_by_value)]
pub fn stale(
    ws: &Workspace,
    identity: &Identity,
    input: StaleInput,
    _events: &EventBus,
) -> Result<ListOutput, OpsError> {
    let ctx = MemoryCtx::open(ws, identity, "memory stale")?;
    // `stale = true` forces the freshness filter on; all other filters off.
    let rows = scan(&ctx, input.kind, None, None, true, None)?;
    Ok(ListOutput { rows })
}

/// `memory show` op.
#[allow(clippy::needless_pass_by_value)]
pub fn show(
    ws: &Workspace,
    identity: &Identity,
    input: ShowInput,
    _events: &EventBus,
) -> Result<ShowOutput, OpsError> {
    let ctx = MemoryCtx::open(ws, identity, "memory show")?;
    let id = ctx.resolve_id(&input.id)?;
    let record = ctx.read_record(&id)?;
    Ok(ShowOutput { record })
}

fn scan(
    ctx: &MemoryCtx<'_>,
    kind: Option<MemoryKind>,
    trust: Option<TrustStateInput>,
    risk: Option<RiskClassInput>,
    stale_only: bool,
    limit: Option<u64>,
) -> Result<Vec<MemoryRowOut>, OpsError> {
    let mut filter = StorageFilter::default();
    if let Some(k) = kind {
        filter = filter.kind(k.to_core());
    } else {
        for k in MEMORY_KINDS {
            filter = filter.kind(*k);
        }
    }

    let ids = ctx
        .storage
        .list(&filter)
        .map_err(|e| OpsError::Internal(anyhow::anyhow!("list storage: {e}")))?;

    let policy = StalePolicy::default();
    let now = Utc::now();
    let mut rows: Vec<MemoryRowOut> = Vec::new();
    for id in ids {
        let Ok(record) = ctx.storage.read(&id) else {
            continue;
        };
        let row_trust = body_trust(&record.body);
        let row_risk = body_risk(&record.body);
        let row_stale = is_stale(&record, now, &policy);

        if let Some(want) = trust {
            if row_trust.as_ref() != Some(&want.to_core()) {
                continue;
            }
        }
        if let Some(want) = risk {
            if row_risk.as_ref() != Some(&want.to_core()) {
                continue;
            }
        }
        if stale_only && !row_stale {
            continue;
        }
        rows.push(MemoryRowOut::from_record(
            &record, row_trust, row_risk, row_stale,
        ));
        if let Some(cap) = limit {
            if rows.len() as u64 >= cap {
                break;
            }
        }
    }
    Ok(rows)
}

impl MemoryRowOut {
    fn from_record(
        record: &Record,
        trust: Option<TrustState>,
        risk: Option<RiskClass>,
        stale: bool,
    ) -> Self {
        Self {
            id: record.envelope.id.as_str().to_string(),
            kind: serialize_lower(&record.envelope.kind),
            title: record.envelope.title.clone(),
            trust: trust.map(|t| serialize_lower(&t)),
            risk_class: risk.map(|r| serialize_lower(&r)),
            stale,
        }
    }
}

fn serialize_lower<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_value(v)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn body_trust(body: &RecordBody) -> Option<TrustState> {
    match body {
        RecordBody::Incident(b) => Some(b.trust),
        RecordBody::Finding(b) => Some(b.trust),
        RecordBody::Runbook(b) => Some(b.trust),
        RecordBody::Decision(b) => Some(b.trust),
        RecordBody::Gotcha(b) => Some(b.trust),
        RecordBody::Memory(b) => Some(b.trust),
        _ => None,
    }
}

fn body_risk(body: &RecordBody) -> Option<RiskClass> {
    match body {
        RecordBody::Incident(b) => b.risk_class,
        RecordBody::Finding(b) => b.risk_class,
        RecordBody::Runbook(b) => b.risk_class,
        RecordBody::Decision(b) => b.risk_class,
        RecordBody::Gotcha(b) => b.risk_class,
        RecordBody::Memory(b) => b.risk_class,
        _ => None,
    }
}

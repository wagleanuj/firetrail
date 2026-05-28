//! `verify` op — walk per-record history chains and report tampering.
//!
//! Mirrors `ft_cli::commands::verify`. A single-id call returns the verdict
//! for that record; an all-records call walks every record on disk and
//! aggregates per-record results.

use std::path::Path;

use ft_core::Record;
use ft_history::verify_chain;
use ft_storage::{EmbeddedStorage, Storage as _};
use serde::{Deserialize, Serialize};

use crate::error::OpsError;
use crate::events::{Event, EventBus};
use crate::identity::Identity;
use crate::workspace::Workspace;

/// Input for [`verify`].
///
/// When `id` is `Some`, verify only that record; when `None`, walk every
/// record file directly so corrupted records surface as per-record failures
/// rather than aborting the whole pass.
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export, rename = "VerifyInput"))]
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyInput {
    /// Specific record id (full canonical) or `None` for "every record".
    #[serde(default)]
    pub id: Option<String>,
    /// Optional client-supplied correlation id.
    #[serde(default)]
    pub request_id: Option<String>,
}

/// Per-record verdict.
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyResult {
    /// Canonical record id (or file path when the record failed to parse).
    pub id: String,
    /// `true` if the chain verified.
    pub ok: bool,
    /// First failure reason (`None` when `ok`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Output of [`verify`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export, rename = "VerifyOutput"))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyOutput {
    /// Total records inspected.
    pub total: usize,
    /// Records that failed verification.
    pub failures: usize,
    /// Per-record verdicts.
    pub results: Vec<VerifyResult>,
}

/// `verify` op.
#[allow(clippy::needless_pass_by_value)]
pub fn verify(
    ws: &Workspace,
    _identity: &Identity,
    input: VerifyInput,
    events: &EventBus,
) -> Result<VerifyOutput, OpsError> {
    let storage = EmbeddedStorage::open(&ws.root)
        .map_err(|e| OpsError::Internal(anyhow::anyhow!("open storage: {e}")))?;
    let mut results: Vec<VerifyResult> = Vec::new();
    let mut failures = 0usize;

    if let Some(raw) = &input.id {
        let id = ft_core::RecordId::from_string(raw.clone())
            .map_err(|e| OpsError::validation("id", e.to_string()))?;
        let record = storage.read(&id).map_err(|e| match e {
            ft_storage::StorageError::NotFound(_) => OpsError::not_found("memory", raw.clone()),
            other => OpsError::Internal(anyhow::anyhow!("read record: {other}")),
        })?;
        let (ok, reason) = match verify_chain(&record) {
            Ok(()) => (true, None),
            Err(e) => {
                failures += 1;
                (false, Some(e.to_string()))
            }
        };
        results.push(VerifyResult {
            id: id.as_str().to_string(),
            ok,
            reason,
        });
    } else {
        let root = storage.records_root();
        for path in super::lint::walk_records(&root) {
            let (id_str, ok, reason) = inspect_path(&path);
            if !ok {
                failures += 1;
            }
            results.push(VerifyResult {
                id: id_str,
                ok,
                reason,
            });
        }
    }

    let ok = failures == 0;
    let event = Event::VerifyRun { ok };
    if let Some(rid) = input.request_id.as_deref() {
        events.emit_with_request(rid.to_string(), event);
    } else {
        events.emit(event);
    }

    Ok(VerifyOutput {
        total: results.len(),
        failures,
        results,
    })
}

fn inspect_path(path: &Path) -> (String, bool, Option<String>) {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            return (
                path.display().to_string(),
                false,
                Some(format!("read: {e}")),
            );
        }
    };
    let record: Record = match serde_json::from_slice(&bytes) {
        Ok(r) => r,
        Err(e) => {
            return (
                path.display().to_string(),
                false,
                Some(format!("parse: {e}")),
            );
        }
    };
    let id = record.envelope.id.as_str().to_string();

    let recomputed = match ft_core::state_hash(&record) {
        Ok(h) => h,
        Err(e) => return (id, false, Some(format!("hash recompute: {e}"))),
    };
    if recomputed != record.envelope.state_hash {
        return (
            id,
            false,
            Some(format!(
                "state_hash mismatch: stored={} recomputed={}",
                record.envelope.state_hash, recomputed
            )),
        );
    }
    match verify_chain(&record) {
        Ok(()) => (id, true, None),
        Err(e) => (id, false, Some(e.to_string())),
    }
}

//! `identity offboard` op.
//!
//! Flips an identity's status to `offboarded` and, when `sweep_claims` is set,
//! walks every record and releases the claims held by the identity. Mirrors
//! the CLI's behaviour (`commands::identity::offboard`).

use chrono::Utc;
use ft_core::RecordBody;
use ft_identity::{find_live_claims_for, load_registry};
use ft_storage::{Storage as _, StorageFilter};
use serde::{Deserialize, Serialize};

use crate::error::OpsError;
use crate::events::EventBus;
use crate::identity::Identity;
use crate::tickets;
use crate::workspace::Workspace;

use super::register::emit_updated;

/// Input for [`offboard`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export, rename = "IdentityOffboardInput"))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OffboardInput {
    /// Identity id to offboard.
    pub id: String,
    /// Whether to walk every record and release the identity's claims.
    #[serde(default)]
    pub sweep_claims: bool,
    /// Optional client-supplied correlation id.
    #[serde(default)]
    pub request_id: Option<String>,
}

/// Output of [`offboard`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export, rename = "IdentityOffboardOutput"))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityOffboardOutput {
    /// The identity that was offboarded.
    pub id: String,
    /// Whether `sweep_claims` was requested.
    pub swept: bool,
    /// Record ids whose claims were released.
    pub released_claims: Vec<String>,
    /// Non-fatal warnings encountered during the sweep.
    pub warnings: Vec<String>,
}

/// `identity offboard` op.
pub fn offboard(
    ws: &Workspace,
    caller: &Identity,
    input: OffboardInput,
    events: &EventBus,
) -> Result<IdentityOffboardOutput, OpsError> {
    let mut registry = load_registry(&ws.root)
        .map_err(|e| OpsError::Internal(anyhow::anyhow!("load registry: {e}")))?;
    let aliases: Vec<String> = registry
        .identities
        .iter()
        .find(|i| i.id == input.id)
        .map(|i| i.emails.clone())
        .ok_or_else(|| OpsError::not_found("identity", input.id.clone()))?;
    registry
        .offboard(&input.id)
        .map_err(|e| OpsError::Internal(anyhow::anyhow!("offboard: {e}")))?;
    registry
        .save(&ws.root)
        .map_err(|e| OpsError::Internal(anyhow::anyhow!("save registry: {e}")))?;

    let mut released: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    if input.sweep_claims {
        // We need write access to records to release claims. Reuse the ticket
        // ctx machinery (history-aware save + index/search refresh) rather
        // than touching storage directly — same code path the CLI follows.
        let mut ctx = tickets::ctx_for_offboard(ws, caller)?;
        let ids = ctx
            .storage
            .list(&StorageFilter::default())
            .map_err(|e| OpsError::Internal(anyhow::anyhow!("list storage: {e}")))?;
        let mut targets: Vec<String> = aliases;
        targets.push(input.id.clone());

        let mut records = Vec::with_capacity(ids.len());
        for id in &ids {
            match ctx.storage.read(id) {
                Ok(r) => records.push(r),
                Err(e) => warnings.push(format!("could not read {id} during sweep: {e}")),
            }
        }
        let mut to_release: Vec<ft_core::RecordId> = Vec::new();
        for target in &targets {
            for rid in find_live_claims_for(records.clone(), target) {
                if !to_release.iter().any(|x| x == &rid) {
                    to_release.push(rid);
                }
            }
        }

        for rid in to_release {
            let mut record = match ctx.read_record(&rid) {
                Ok(r) => r,
                Err(e) => {
                    warnings.push(format!("could not re-read {rid} for release: {e}"));
                    continue;
                }
            };
            clear_claim(&mut record.body);
            record.envelope.updated_at = Utc::now();
            match ctx.save_record(&mut record) {
                Ok(_) => released.push(rid.as_str().to_string()),
                Err(e) => warnings.push(format!("could not release claim on {rid}: {e}")),
            }
        }
    }

    let mut fields: Vec<&'static str> = vec!["status"];
    if input.sweep_claims {
        fields.push("claims_released");
    }
    emit_updated(events, input.request_id.as_deref(), &input.id, &fields);

    Ok(IdentityOffboardOutput {
        id: input.id,
        swept: input.sweep_claims,
        released_claims: released,
        warnings,
    })
}

fn clear_claim(body: &mut RecordBody) {
    match body {
        RecordBody::Task(t) => t.claim = None,
        RecordBody::Subtask(s) => s.claim = None,
        RecordBody::Bug(b) => b.claim = None,
        _ => {}
    }
}

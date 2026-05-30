//! Update an identity's capability overrides.
//!
//! Mirrors the `register` write path. Accepts a list of overrides where each
//! entry has a key and a tri-state value (`None` clears the override; `Some`
//! sets `allow`/`deny`). Emits [`crate::Event::IdentityUpdated`] on success.

use ft_identity::load_registry;
use serde::{Deserialize, Serialize};

use crate::error::OpsError;
use crate::events::EventBus;
use crate::identity::Identity;
use crate::workspace::Workspace;

use super::register::emit_updated;
use super::views::IdentityView;

/// One capability override patch on an update input.
///
/// `value = None` clears the override (falls back to kind defaults).
/// `value = Some(true)` allows; `value = Some(false)` denies.
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityPatch {
    /// Capability key (e.g. `can_promote_verified`).
    pub key: String,
    /// New value, or `None` to clear the override.
    #[serde(default)]
    pub value: Option<bool>,
}

/// Input for [`update_capabilities`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts-rs",
    ts(export, rename = "IdentityUpdateCapabilitiesInput")
)]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCapabilitiesInput {
    /// Identity id.
    pub id: String,
    /// Patches to apply.
    pub capabilities: Vec<CapabilityPatch>,
    /// Optional client-supplied correlation id.
    #[serde(default)]
    pub request_id: Option<String>,
}

/// Output of [`update_capabilities`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts-rs",
    ts(export, rename = "IdentityUpdateCapabilitiesOutput")
)]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCapabilitiesOutput {
    /// The updated identity view.
    pub identity: IdentityView,
}

/// `identity update-capabilities` op.
#[allow(clippy::needless_pass_by_value)]
pub fn update_capabilities(
    ws: &Workspace,
    _caller: &Identity,
    input: UpdateCapabilitiesInput,
    events: &EventBus,
) -> Result<UpdateCapabilitiesOutput, OpsError> {
    let mut registry = load_registry(&ws.root)
        .map_err(|e| OpsError::Internal(anyhow::anyhow!("load registry: {e}")))?;
    let ident = registry
        .identities
        .iter_mut()
        .find(|i| i.id == input.id)
        .ok_or_else(|| OpsError::not_found("identity", input.id.clone()))?;

    for patch in &input.capabilities {
        let key = patch.key.trim();
        if key.is_empty() {
            return Err(OpsError::validation("capabilities", "empty capability key"));
        }
        match key {
            "can_promote_verified" => ident.capabilities.can_promote_verified = patch.value,
            "can_close_high_risk" => ident.capabilities.can_close_high_risk = patch.value,
            "can_force_push" => ident.capabilities.can_force_push = patch.value,
            "can_redact" => ident.capabilities.can_redact = patch.value,
            other => match patch.value {
                Some(v) => {
                    ident.capabilities.extra.insert(other.to_string(), v);
                }
                None => {
                    ident.capabilities.extra.remove(other);
                }
            },
        }
    }
    let view = IdentityView::from(&*ident);
    registry
        .save(&ws.root)
        .map_err(|e| OpsError::Internal(anyhow::anyhow!("save registry: {e}")))?;

    // firetrail-8z0m.5: re-embed the `identity:<id>` synthetic doc on write so
    // an updated capability matrix is reflected in semantic search immediately.
    if let Some(saved) = registry.identities.iter().find(|i| i.id == input.id) {
        crate::synthetic_embed::dispatch_identity(ws, "identity update", saved);
    }

    emit_updated(
        events,
        input.request_id.as_deref(),
        &input.id,
        &["capabilities"],
    );
    Ok(UpdateCapabilitiesOutput { identity: view })
}

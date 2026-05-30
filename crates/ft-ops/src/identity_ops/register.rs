//! Identity-registry write ops (`register`) plus the capability-matrix view.
//!
//! Emits [`crate::Event::IdentityUpdated`] on every successful write.

use ft_identity::{
    CapabilityMatrix, IdentityKind, IdentityStatus, PartialCapabilityMatrix, RegisteredIdentity,
    load_registry,
};
use serde::{Deserialize, Serialize};

use crate::error::OpsError;
use crate::events::{Event, EventBus};
use crate::identity::Identity;
use crate::workspace::Workspace;

use super::views::{IdentityView, kind_str};

/// Identity-kind selector for [`register`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum IdentityKindInput {
    /// Real person.
    Human,
    /// Service / bot account.
    Bot,
    /// CI runner.
    Ci,
}

impl IdentityKindInput {
    fn to_core(self) -> IdentityKind {
        match self {
            Self::Human => IdentityKind::Human,
            Self::Bot => IdentityKind::Bot,
            Self::Ci => IdentityKind::Ci,
        }
    }
}

/// Input for [`register`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export, rename = "IdentityRegisterInput"))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterInput {
    /// Canonical short id (e.g. `alice`, `bot-claude`).
    pub id: String,
    /// Human-readable display name.
    pub name: String,
    /// Email aliases (must be non-empty).
    pub emails: Vec<String>,
    /// Identity kind.
    pub kind: IdentityKindInput,
    /// Machine hostnames.
    #[serde(default)]
    pub machines: Vec<String>,
    /// `key=value` capability overrides parsed by the caller. The CLI splits
    /// `--capability key=value` flags; the GUI submits an already-parsed
    /// list.
    #[serde(default)]
    pub capabilities: Vec<CapabilityOverrideInput>,
    /// Optional client-supplied correlation id.
    #[serde(default)]
    pub request_id: Option<String>,
}

/// One capability override on a register input.
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityOverrideInput {
    /// Capability key.
    pub key: String,
    /// Override value.
    pub value: bool,
}

/// Output of [`register`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export, rename = "IdentityRegisterOutput"))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityRegisterOutput {
    /// The identity that was written.
    pub identity: IdentityView,
}

/// `identity register` op.
pub fn register(
    ws: &Workspace,
    _identity: &Identity,
    input: RegisterInput,
    events: &EventBus,
) -> Result<IdentityRegisterOutput, OpsError> {
    let mut registry = load_registry(&ws.root)
        .map_err(|e| OpsError::Internal(anyhow::anyhow!("load registry: {e}")))?;
    if registry.identities.iter().any(|i| i.id == input.id) {
        return Err(OpsError::conflict(format!(
            "identity `{}` is already registered",
            input.id
        )));
    }

    let emails: Vec<String> = input
        .emails
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if emails.is_empty() {
        return Err(OpsError::validation(
            "emails",
            "at least one email is required",
        ));
    }
    let machines: Vec<String> = input
        .machines
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let mut capabilities = PartialCapabilityMatrix::default();
    for entry in input.capabilities {
        let key = entry.key.trim().to_string();
        if key.is_empty() {
            return Err(OpsError::validation("capabilities", "empty capability key"));
        }
        match key.as_str() {
            "can_promote_verified" => capabilities.can_promote_verified = Some(entry.value),
            "can_close_high_risk" => capabilities.can_close_high_risk = Some(entry.value),
            "can_force_push" => capabilities.can_force_push = Some(entry.value),
            "can_redact" => capabilities.can_redact = Some(entry.value),
            _ => {
                capabilities.extra.insert(key, entry.value);
            }
        }
    }

    let new = RegisteredIdentity {
        id: input.id.clone(),
        name: input.name,
        kind: input.kind.to_core(),
        emails,
        machines,
        capabilities,
        status: IdentityStatus::Active,
    };
    let view = IdentityView::from(&new);
    registry.identities.push(new);
    registry
        .save(&ws.root)
        .map_err(|e| OpsError::Internal(anyhow::anyhow!("save registry: {e}")))?;

    // firetrail-8z0m.5: re-embed the `identity:<id>` synthetic doc on write so
    // semantic search reflects the new identity immediately (best-effort,
    // daemon-gated, non-fatal — same policy as record on-write dispatch).
    if let Some(saved) = registry.identities.iter().find(|i| i.id == input.id) {
        crate::synthetic_embed::dispatch_identity(ws, "identity register", saved);
    }

    emit_updated(events, input.request_id.as_deref(), &input.id, &["create"]);
    Ok(IdentityRegisterOutput { identity: view })
}

// ─────────────────────────────────────────────────────────────────────────────
// capabilities — surface the effective matrix for a registered identity.
// ─────────────────────────────────────────────────────────────────────────────

/// Input for [`capabilities`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export, rename = "IdentityCapabilitiesInput"))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilitiesInput {
    /// Identity id to look up.
    pub id: String,
    /// Optional client-supplied correlation id.
    #[serde(default)]
    pub request_id: Option<String>,
}

/// One row in the capability matrix.
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityRow {
    /// Capability key.
    pub capability: String,
    /// Whether the identity holds it after defaults + overrides.
    pub granted: bool,
    /// Whether the value came from an explicit override (`true`) or the
    /// kind default (`false`).
    pub overridden: bool,
}

/// Output of [`capabilities`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export, rename = "IdentityCapabilitiesOutput"))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilitiesOutput {
    /// Identity id.
    pub identity: String,
    /// Identity kind (`human` / `bot` / `ci`).
    pub kind: String,
    /// Lifecycle status.
    pub status: String,
    /// Effective capability matrix (defaults + overrides composed).
    pub capabilities: Vec<CapabilityRow>,
}

/// `identity capabilities` op (read-only; surfaces the composed matrix).
#[allow(clippy::needless_pass_by_value)]
pub fn capabilities(
    ws: &Workspace,
    _identity: &Identity,
    input: CapabilitiesInput,
    _events: &EventBus,
) -> Result<CapabilitiesOutput, OpsError> {
    let registry = load_registry(&ws.root)
        .map_err(|e| OpsError::Internal(anyhow::anyhow!("load registry: {e}")))?;
    let ident = registry
        .identities
        .iter()
        .find(|i| i.id == input.id)
        .ok_or_else(|| OpsError::not_found("identity", input.id.clone()))?;

    let defaults = CapabilityMatrix::defaults_for_kind(ident.kind);
    let effective = ident.effective_capabilities();
    let mut rows: Vec<CapabilityRow> = Vec::new();

    let well_known = [
        (
            "can_promote_verified",
            defaults.can_promote_verified,
            effective.can_promote_verified,
            ident.capabilities.can_promote_verified.is_some(),
        ),
        (
            "can_close_high_risk",
            defaults.can_close_high_risk,
            effective.can_close_high_risk,
            ident.capabilities.can_close_high_risk.is_some(),
        ),
        (
            "can_force_push",
            defaults.can_force_push,
            effective.can_force_push,
            ident.capabilities.can_force_push.is_some(),
        ),
        (
            "can_redact",
            defaults.can_redact,
            effective.can_redact,
            ident.capabilities.can_redact.is_some(),
        ),
    ];
    for (name, _default_v, effective_v, overridden) in well_known {
        rows.push(CapabilityRow {
            capability: name.to_string(),
            granted: effective_v,
            overridden,
        });
    }
    // Extras present an explicit per-team capability — always overridden.
    let mut extras: Vec<(&String, &bool)> = effective.extra.iter().collect();
    extras.sort_by(|a, b| a.0.cmp(b.0));
    for (k, v) in extras {
        rows.push(CapabilityRow {
            capability: k.clone(),
            granted: *v,
            overridden: true,
        });
    }

    Ok(CapabilitiesOutput {
        identity: ident.id.clone(),
        kind: kind_str(ident.kind).into(),
        status: super::views::status_str(ident.status).into(),
        capabilities: rows,
    })
}

pub(super) fn emit_updated(bus: &EventBus, request_id: Option<&str>, id: &str, fields: &[&str]) {
    let event = Event::IdentityUpdated {
        identity: id.to_string(),
        fields: fields.iter().map(|s| (*s).to_string()).collect(),
    };
    if let Some(rid) = request_id {
        bus.emit_with_request(rid.to_string(), event);
    } else {
        bus.emit(event);
    }
}

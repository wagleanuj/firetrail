//! Read-only identity-registry views: `list` and `show`.

use ft_identity::{IdentityKind, IdentityStatus, RegisteredIdentity, load_registry};
use serde::{Deserialize, Serialize};

use crate::error::OpsError;
use crate::events::EventBus;
use crate::identity::Identity;
use crate::workspace::Workspace;

/// Status filter for [`list`]. Mirrors `ft_identity::IdentityStatus`.
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum IdentityStatusFilter {
    /// Active members.
    Active,
    /// Offboarded.
    Offboarded,
}

impl IdentityStatusFilter {
    pub(crate) fn matches(self, s: IdentityStatus) -> bool {
        matches!(
            (self, s),
            (Self::Active, IdentityStatus::Active)
                | (Self::Offboarded, IdentityStatus::Offboarded)
        )
    }
}

/// Wire-friendly view of a single registered identity.
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityView {
    /// Canonical id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Identity kind (`human` / `bot` / `ci`).
    pub kind: String,
    /// Lifecycle status (`active` / `offboarded`).
    pub status: String,
    /// Email aliases.
    pub emails: Vec<String>,
    /// Machine hostnames.
    pub machines: Vec<String>,
    /// Capability overrides as a flat key/value list (only keys explicitly
    /// set in the registry; defaults are not flattened here — see
    /// [`super::capabilities`] for the effective matrix).
    pub capabilities: Vec<CapabilityOverride>,
}

/// One `key=value` capability override on an identity record.
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityOverride {
    /// Capability key (e.g. `can_promote_verified`).
    pub key: String,
    /// Override value.
    pub value: bool,
}

impl IdentityView {
    pub(crate) fn from(reg: &RegisteredIdentity) -> Self {
        let mut caps: Vec<CapabilityOverride> = Vec::new();
        if let Some(v) = reg.capabilities.can_promote_verified {
            caps.push(CapabilityOverride {
                key: "can_promote_verified".into(),
                value: v,
            });
        }
        if let Some(v) = reg.capabilities.can_close_high_risk {
            caps.push(CapabilityOverride {
                key: "can_close_high_risk".into(),
                value: v,
            });
        }
        if let Some(v) = reg.capabilities.can_force_push {
            caps.push(CapabilityOverride {
                key: "can_force_push".into(),
                value: v,
            });
        }
        if let Some(v) = reg.capabilities.can_redact {
            caps.push(CapabilityOverride {
                key: "can_redact".into(),
                value: v,
            });
        }
        let mut extras: Vec<_> = reg
            .capabilities
            .extra
            .iter()
            .map(|(k, v)| CapabilityOverride {
                key: k.clone(),
                value: *v,
            })
            .collect();
        extras.sort_by(|a, b| a.key.cmp(&b.key));
        caps.extend(extras);

        Self {
            id: reg.id.clone(),
            name: reg.name.clone(),
            kind: kind_str(reg.kind).into(),
            status: status_str(reg.status).into(),
            emails: reg.emails.clone(),
            machines: reg.machines.clone(),
            capabilities: caps,
        }
    }
}

pub(crate) fn kind_str(k: IdentityKind) -> &'static str {
    match k {
        IdentityKind::Human => "human",
        IdentityKind::Bot => "bot",
        IdentityKind::Ci => "ci",
    }
}

pub(crate) fn status_str(s: IdentityStatus) -> &'static str {
    match s {
        IdentityStatus::Active => "active",
        IdentityStatus::Offboarded => "offboarded",
    }
}

/// Input for [`list`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export, rename = "IdentityListInput"))]
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListInput {
    /// Filter by lifecycle status.
    #[serde(default)]
    pub status: Option<IdentityStatusFilter>,
    /// Optional client-supplied correlation id.
    #[serde(default)]
    pub request_id: Option<String>,
}

/// Output of [`list`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export, rename = "IdentityListOutput"))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityListOutput {
    /// Identities, in registry order.
    pub identities: Vec<IdentityView>,
}

/// Input for [`show`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export, rename = "IdentityShowInput"))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShowInput {
    /// Canonical id.
    pub id: String,
    /// Optional client-supplied correlation id.
    #[serde(default)]
    pub request_id: Option<String>,
}

/// Output of [`show`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export, rename = "IdentityShowOutput"))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityShowOutput {
    /// The resolved identity.
    pub identity: IdentityView,
}

/// `identity list` op.
#[allow(clippy::needless_pass_by_value)]
pub fn list(
    ws: &Workspace,
    _identity: &Identity,
    input: ListInput,
    _events: &EventBus,
) -> Result<IdentityListOutput, OpsError> {
    let registry = load_registry(&ws.root)
        .map_err(|e| OpsError::Internal(anyhow::anyhow!("load registry: {e}")))?;
    let filter = input.status;
    let identities: Vec<IdentityView> = registry
        .identities
        .iter()
        .filter(|i| filter.is_none_or(|f| f.matches(i.status)))
        .map(IdentityView::from)
        .collect();
    Ok(IdentityListOutput { identities })
}

/// `identity show` op.
#[allow(clippy::needless_pass_by_value)]
pub fn show(
    ws: &Workspace,
    _identity: &Identity,
    input: ShowInput,
    _events: &EventBus,
) -> Result<IdentityShowOutput, OpsError> {
    let registry = load_registry(&ws.root)
        .map_err(|e| OpsError::Internal(anyhow::anyhow!("load registry: {e}")))?;
    let ident = registry
        .identities
        .iter()
        .find(|i| i.id == input.id)
        .ok_or_else(|| OpsError::not_found("identity", input.id.clone()))?;
    Ok(IdentityShowOutput {
        identity: IdentityView::from(ident),
    })
}

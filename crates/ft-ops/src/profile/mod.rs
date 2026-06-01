//! Transport-agnostic repo-profile surface (`RepoProfile` epic).
//!
//! A repo has at most one [`ft_core::RepoProfileBody`] record — a small bag of
//! always-read facts: the validate/test/build/lint commands, language/tooling
//! facts, and a shallow component map. The agent inspects the repo and decides
//! these (ADR-0005); firetrail only stores, indexes, and surfaces them.
//!
//! These ops back the ft-ui Profile panel:
//!
//! - [`get`] — read the current profile (or `None`).
//! - [`update`] — partial update: `Option` fields overwrite when `Some`, vec
//!   fields overwrite when present. Load-or-create; the edited body stays
//!   [`TrustState::Draft`] (trust transitions go through `ft-trust`).
//! - [`add_component`] / [`remove_component`] — manage the component map.
//!
//! Like every op in this crate these are embedded-storage only and take
//! `(&Workspace, &Identity, Input, &EventBus)`. The singleton read/upsert
//! convention lives in [`ft_storage::profile_get`]; here we drive it through
//! [`TicketCtx`] so the index + history stay current.

pub mod resolve;

use ft_core::{ComponentRef, Record, RecordBody, RecordBuilder, RecordKind, RepoProfileBody};
use serde::{Deserialize, Serialize};

use crate::error::OpsError;
use crate::events::{Event, EventBus};
use crate::identity::Identity;
use crate::tickets::ctx::TicketCtx;
use crate::workspace::Workspace;

// ─────────────────────────────────────────────────────────────────────────────
// Wire types.
// ─────────────────────────────────────────────────────────────────────────────

/// A component reference as the Profile panel renders it.
///
/// The wire mirror of [`ft_core::ComponentRef`], kept in `ft-ops` so ts-rs only
/// ever sees ops types.
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComponentView {
    /// Human-readable component name (e.g. `ft-cli`).
    pub name: String,
    /// Repo-relative path to the component.
    pub path: String,
    /// Optional one-line summary.
    pub summary: Option<String>,
}

impl From<&ComponentRef> for ComponentView {
    fn from(c: &ComponentRef) -> Self {
        Self {
            name: c.name.clone(),
            path: c.path.clone(),
            summary: c.summary.clone(),
        }
    }
}

/// The repo profile as the Profile panel renders it: the record id, every
/// command/tooling field, the component map, notes, and the trust state (as a
/// serialized lowercase string, e.g. `"draft"`).
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileView {
    /// Canonical profile record id.
    pub id: String,
    /// The canonical "prove a change is good" command.
    pub validate_command: Option<String>,
    /// Standard test command.
    pub test_command: Option<String>,
    /// Standard build command.
    pub build_command: Option<String>,
    /// Standard lint command.
    pub lint_command: Option<String>,
    /// Primary language(s).
    pub languages: Vec<String>,
    /// Package manager(s).
    pub package_managers: Vec<String>,
    /// Optional runtime note.
    pub runtime: Option<String>,
    /// Shallow component map (names + paths only).
    pub components: Vec<ComponentView>,
    /// Free-form notes.
    pub notes: Option<String>,
    /// Trust state (lowercase, e.g. `"draft"`, `"reviewed"`, `"verified"`).
    pub trust: String,
}

impl ProfileView {
    fn from_record(record: &Record, body: &RepoProfileBody) -> Self {
        Self {
            id: record.envelope.id.as_str().to_string(),
            validate_command: body.validate_command.clone(),
            test_command: body.test_command.clone(),
            build_command: body.build_command.clone(),
            lint_command: body.lint_command.clone(),
            languages: body.languages.clone(),
            package_managers: body.package_managers.clone(),
            runtime: body.runtime.clone(),
            components: body.components.iter().map(ComponentView::from).collect(),
            notes: body.notes.clone(),
            trust: trust_str(body.trust),
        }
    }
}

/// Serialize a [`ft_core::TrustState`] to its lowercase wire form.
fn trust_str(t: ft_core::TrustState) -> String {
    serde_json::to_value(t)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| format!("{t:?}").to_lowercase())
}

/// Input for [`update`] — partial update of the profile.
///
/// The `Option<Option<T>>` fields are deliberate: the outer layer distinguishes
/// "field untouched" (`None`) from "set / clear it" (`Some(..)`), which a flat
/// `Option<T>` cannot express for a partial update of nullable fields. Vec
/// fields overwrite the corresponding slice when present; `None` leaves it
/// untouched.
#[allow(clippy::option_option)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProfileInput {
    /// New validate command (`Some(None)` clears it).
    #[serde(default)]
    pub validate_command: Option<Option<String>>,
    /// New test command.
    #[serde(default)]
    pub test_command: Option<Option<String>>,
    /// New build command.
    #[serde(default)]
    pub build_command: Option<Option<String>>,
    /// New lint command.
    #[serde(default)]
    pub lint_command: Option<Option<String>>,
    /// Replacement language list.
    #[serde(default)]
    pub languages: Option<Vec<String>>,
    /// Replacement package-manager list.
    #[serde(default)]
    pub package_managers: Option<Vec<String>>,
    /// New runtime note.
    #[serde(default)]
    pub runtime: Option<Option<String>>,
    /// New free-form notes.
    #[serde(default)]
    pub notes: Option<Option<String>>,
    /// Optional client-supplied correlation id.
    #[serde(default)]
    pub request_id: Option<String>,
}

/// Input for [`add_component`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddComponentInput {
    /// Component name (unique key in the map).
    pub name: String,
    /// Repo-relative path.
    pub path: String,
    /// Optional one-line summary.
    #[serde(default)]
    pub summary: Option<String>,
    /// Optional client-supplied correlation id.
    #[serde(default)]
    pub request_id: Option<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Read.
// ─────────────────────────────────────────────────────────────────────────────

/// Read the current repo profile, or `None` when no profile record exists.
#[allow(clippy::needless_pass_by_value)]
pub fn get(
    ws: &Workspace,
    identity: &Identity,
    _events: &EventBus,
) -> Result<Option<ProfileView>, OpsError> {
    let ctx = TicketCtx::open(ws, identity, "profile show")?;
    let Some(record) = ft_storage::profile_get(&ctx.storage)
        .map_err(|e| OpsError::Internal(anyhow::anyhow!("read profile: {e}")))?
    else {
        return Ok(None);
    };
    let RecordBody::RepoProfile(body) = &record.body else {
        return Err(OpsError::Internal(anyhow::anyhow!(
            "profile record {} is not a RepoProfile body",
            record.envelope.id.as_str()
        )));
    };
    Ok(Some(ProfileView::from_record(&record, body)))
}

// ─────────────────────────────────────────────────────────────────────────────
// Write.
// ─────────────────────────────────────────────────────────────────────────────

/// Partial-update the profile, creating it if absent.
///
/// `Option` fields overwrite when `Some`, vec fields overwrite when present;
/// every other field is preserved. The edited body stays
/// [`ft_core::TrustState::Draft`] — trust transitions go through `ft-trust`,
/// never here.
#[allow(clippy::needless_pass_by_value)]
pub fn update(
    ws: &Workspace,
    identity: &Identity,
    input: UpdateProfileInput,
    events: &EventBus,
) -> Result<ProfileView, OpsError> {
    with_profile(
        ws,
        identity,
        "profile set",
        input.request_id.clone(),
        events,
        |body| {
            if let Some(v) = input.validate_command {
                body.validate_command = v;
            }
            if let Some(v) = input.test_command {
                body.test_command = v;
            }
            if let Some(v) = input.build_command {
                body.build_command = v;
            }
            if let Some(v) = input.lint_command {
                body.lint_command = v;
            }
            if let Some(v) = input.languages {
                body.languages = v;
            }
            if let Some(v) = input.package_managers {
                body.package_managers = v;
            }
            if let Some(v) = input.runtime {
                body.runtime = v;
            }
            if let Some(v) = input.notes {
                body.notes = v;
            }
            Ok(())
        },
    )
}

/// Add (or replace, by name) a component in the shallow component map.
#[allow(clippy::needless_pass_by_value)]
pub fn add_component(
    ws: &Workspace,
    identity: &Identity,
    input: AddComponentInput,
    events: &EventBus,
) -> Result<ProfileView, OpsError> {
    if input.name.trim().is_empty() {
        return Err(OpsError::validation("name", "component name is required"));
    }
    if input.path.trim().is_empty() {
        return Err(OpsError::validation("path", "component path is required"));
    }
    with_profile(
        ws,
        identity,
        "profile component add",
        input.request_id.clone(),
        events,
        |body| {
            let component = ComponentRef {
                name: input.name.clone(),
                path: input.path.clone(),
                summary: input.summary.clone(),
            };
            // Replace in place if a component with this name already exists,
            // else append — keeps the map a true set keyed on name.
            if let Some(slot) = body.components.iter_mut().find(|c| c.name == input.name) {
                *slot = component;
            } else {
                body.components.push(component);
            }
            Ok(())
        },
    )
}

/// Remove the component named `name` from the map.
///
/// Errors with [`OpsError::NotFound`] when no component matches (and when no
/// profile exists at all) so the caller can surface a 404.
#[allow(clippy::needless_pass_by_value)]
pub fn remove_component(
    ws: &Workspace,
    identity: &Identity,
    name: String,
    events: &EventBus,
) -> Result<ProfileView, OpsError> {
    // Removing from an absent profile is a 404, not a silent create.
    {
        let ctx = TicketCtx::open(ws, identity, "profile component rm")?;
        if ft_storage::profile_get(&ctx.storage)
            .map_err(|e| OpsError::Internal(anyhow::anyhow!("read profile: {e}")))?
            .is_none()
        {
            return Err(OpsError::not_found("profile", "<none>"));
        }
    }
    with_profile(ws, identity, "profile component rm", None, events, |body| {
        let before = body.components.len();
        body.components.retain(|c| c.name != name);
        if body.components.len() == before {
            return Err(OpsError::not_found("component", name.clone()));
        }
        Ok(())
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Shared load-or-create + mutate + persist.
// ─────────────────────────────────────────────────────────────────────────────

/// Load the profile (or build a fresh `Draft` one), apply `mutate` to its body,
/// persist through [`TicketCtx::save_record`] (index + history refresh), emit a
/// [`Event::ProfileUpdated`], and return the refreshed view.
fn with_profile(
    ws: &Workspace,
    identity: &Identity,
    op: &'static str,
    request_id: Option<String>,
    events: &EventBus,
    mutate: impl FnOnce(&mut RepoProfileBody) -> Result<(), OpsError>,
) -> Result<ProfileView, OpsError> {
    let mut ctx = TicketCtx::open(ws, identity, op)?;

    let mut record = match ft_storage::profile_get(&ctx.storage)
        .map_err(|e| OpsError::Internal(anyhow::anyhow!("read profile: {e}")))?
    {
        Some(record) => record,
        None => RecordBuilder::new(RecordKind::RepoProfile, "Repo profile", ctx.actor.clone())
            .repo_profile(RepoProfileBody::default())
            .build()
            .map_err(|e| OpsError::Internal(anyhow::anyhow!("build profile: {e}")))?,
    };

    let RecordBody::RepoProfile(body) = &mut record.body else {
        return Err(OpsError::Internal(anyhow::anyhow!(
            "profile record {} is not a RepoProfile body",
            record.envelope.id.as_str()
        )));
    };
    mutate(body)?;

    ctx.save_record(&mut record)?;

    let id = record.envelope.id.as_str().to_string();
    match request_id {
        Some(rid) => events.emit_with_request(rid, Event::ProfileUpdated { id: id.clone() }),
        None => events.emit(Event::ProfileUpdated { id: id.clone() }),
    }

    let RecordBody::RepoProfile(body) = &record.body else {
        unreachable!("body was a RepoProfile above");
    };
    Ok(ProfileView::from_record(&record, body))
}

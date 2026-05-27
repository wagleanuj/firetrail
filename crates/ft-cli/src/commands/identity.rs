//! `firetrail identity …` — identity registry management (M5).
//!
//! The registry on disk is `.firetrail/identities.yaml`. Every subcommand
//! loads the file, mutates / inspects it, and persists with [`IdentityRegistry::save`].
//!
//! ## Subcommands
//!
//! - `register <id> --name … --emails … --kind …` — append a new identity.
//! - `list [--status …]` — list configured identities.
//! - `show <id>` — print one identity's full record.
//! - `offboard <id> [--sweep-claims]` — flip status to `offboarded`; with
//!   `--sweep-claims`, walk every record and release claims held by the id.

use std::fmt::Write as _;
use std::path::PathBuf;

use ft_core::RecordBody;
use ft_identity::{
    IdentityKind, IdentityStatus, PartialCapabilityMatrix, REGISTRY_FILENAME, RegisteredIdentity,
    find_live_claims_for, load_registry,
};
use ft_storage::{Storage as _, StorageFilter};
use serde::Serialize;

use crate::cli::{
    GlobalOpts, IdentityListArgs, IdentityOffboardArgs, IdentityRegisterArgs, IdentityShowArgs,
    IdentityStatusArg,
};
use crate::commands::CommandOutcome;
use crate::context::WorkCtx;
use crate::error::CliError;

const CMD_REGISTER: &str = "identity register";
const CMD_LIST: &str = "identity list";
const CMD_SHOW: &str = "identity show";
const CMD_OFFBOARD: &str = "identity offboard";

// ── Outcomes ───────────────────────────────────────────────────────────────

/// Outcome of `identity register`.
#[derive(Debug, Clone, Serialize)]
pub struct IdentityRegisterOutcome {
    /// Stable command name.
    #[serde(skip)]
    pub command: &'static str,
    /// The identity that was written.
    pub identity: SerializedIdentity,
    /// Path to the registry file.
    pub path: PathBuf,
    /// Non-fatal warnings.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<String>,
}

/// Outcome of `identity list`.
#[derive(Debug, Clone, Serialize)]
pub struct IdentityListOutcome {
    /// Stable command name.
    #[serde(skip)]
    pub command: &'static str,
    /// Each identity in registry order.
    pub identities: Vec<SerializedIdentity>,
    /// Non-fatal warnings.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<String>,
}

/// Outcome of `identity show`.
#[derive(Debug, Clone, Serialize)]
pub struct IdentityShowOutcome {
    /// Stable command name.
    #[serde(skip)]
    pub command: &'static str,
    /// The identity in question.
    pub identity: SerializedIdentity,
    /// Non-fatal warnings.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<String>,
}

/// Outcome of `identity offboard`.
#[derive(Debug, Clone, Serialize)]
pub struct IdentityOffboardOutcome {
    /// Stable command name.
    #[serde(skip)]
    pub command: &'static str,
    /// The identity that was offboarded.
    pub id: String,
    /// Whether `--sweep-claims` was requested.
    pub swept: bool,
    /// Record ids whose claims were released.
    pub released_claims: Vec<String>,
    /// Non-fatal warnings.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<String>,
}

/// Serializable view of a single registered identity.
#[derive(Debug, Clone, Serialize)]
pub struct SerializedIdentity {
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
    /// Capability overrides as a flat key/value map (only the keys explicitly
    /// set in the registry are reported; defaults are not flattened).
    pub capabilities: std::collections::BTreeMap<String, bool>,
}

impl SerializedIdentity {
    fn from(reg: &RegisteredIdentity) -> Self {
        let mut caps = std::collections::BTreeMap::new();
        if let Some(v) = reg.capabilities.can_promote_verified {
            caps.insert("can_promote_verified".into(), v);
        }
        if let Some(v) = reg.capabilities.can_close_high_risk {
            caps.insert("can_close_high_risk".into(), v);
        }
        if let Some(v) = reg.capabilities.can_force_push {
            caps.insert("can_force_push".into(), v);
        }
        if let Some(v) = reg.capabilities.can_redact {
            caps.insert("can_redact".into(), v);
        }
        for (k, v) in &reg.capabilities.extra {
            caps.insert(k.clone(), *v);
        }
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

fn kind_str(k: IdentityKind) -> &'static str {
    match k {
        IdentityKind::Human => "human",
        IdentityKind::Bot => "bot",
        IdentityKind::Ci => "ci",
    }
}

fn status_str(s: IdentityStatus) -> &'static str {
    match s {
        IdentityStatus::Active => "active",
        IdentityStatus::Offboarded => "offboarded",
    }
}

// ── Markdown rendering ─────────────────────────────────────────────────────

impl IdentityRegisterOutcome {
    /// Render as markdown.
    #[must_use]
    pub fn markdown(&self) -> String {
        format!(
            "**identity registered** `{}` ({}) — wrote `{}`\n",
            self.identity.id,
            self.identity.kind,
            self.path.display()
        )
    }
    /// One-line summary.
    #[must_use]
    pub fn quiet_line(&self) -> String {
        format!("identity register {}", self.identity.id)
    }
}

impl IdentityListOutcome {
    /// Render as markdown.
    #[must_use]
    pub fn markdown(&self) -> String {
        if self.identities.is_empty() {
            return "_no identities registered_\n".into();
        }
        let mut s = String::from("| id | name | kind | status | emails |\n|---|---|---|---|---|\n");
        for i in &self.identities {
            let _ = writeln!(
                s,
                "| `{}` | {} | {} | {} | {} |",
                i.id,
                i.name,
                i.kind,
                i.status,
                i.emails.join(", ")
            );
        }
        s
    }
    /// One-line summary.
    #[must_use]
    pub fn quiet_line(&self) -> String {
        format!("identity list ({})", self.identities.len())
    }
}

impl IdentityShowOutcome {
    /// Render as markdown.
    #[must_use]
    pub fn markdown(&self) -> String {
        let i = &self.identity;
        let mut s = format!(
            "**identity** `{}` ({}, {})\n\n- name: {}\n- emails: {}\n- machines: {}\n",
            i.id,
            i.kind,
            i.status,
            i.name,
            if i.emails.is_empty() {
                "_none_".to_string()
            } else {
                i.emails.join(", ")
            },
            if i.machines.is_empty() {
                "_none_".to_string()
            } else {
                i.machines.join(", ")
            },
        );
        if !i.capabilities.is_empty() {
            s.push_str("- capabilities:\n");
            for (k, v) in &i.capabilities {
                let _ = writeln!(s, "  - `{k}` = {v}");
            }
        }
        s
    }
    /// One-line summary.
    #[must_use]
    pub fn quiet_line(&self) -> String {
        format!("identity show {}", self.identity.id)
    }
}

impl IdentityOffboardOutcome {
    /// Render as markdown.
    #[must_use]
    pub fn markdown(&self) -> String {
        let mut s = format!("**identity offboarded** `{}`\n", self.id);
        if self.swept {
            let _ = writeln!(s, "\nClaims released: {}", self.released_claims.len());
            for id in &self.released_claims {
                let _ = writeln!(s, "- `{id}`");
            }
        }
        s
    }
    /// One-line summary.
    #[must_use]
    pub fn quiet_line(&self) -> String {
        format!(
            "identity offboard {} ({} claims released)",
            self.id,
            self.released_claims.len()
        )
    }
}

// ── Command handlers ───────────────────────────────────────────────────────

/// `firetrail identity register`
pub fn register(
    args: &IdentityRegisterArgs,
    global: &GlobalOpts,
) -> Result<CommandOutcome, CliError> {
    let ws = crate::workspace::require_initialised(CMD_REGISTER, global.workspace.as_deref())?;
    let mut registry = load_registry(&ws.root)
        .map_err(|e| CliError::internal(CMD_REGISTER, format!("load registry: {e}")))?;
    if registry.identities.iter().any(|i| i.id == args.id) {
        return Err(CliError::UserError {
            command: CMD_REGISTER.into(),
            message: format!("identity `{}` is already registered", args.id),
            details: serde_json::json!({ "id": args.id }),
        });
    }

    let emails: Vec<String> = args
        .emails
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if emails.is_empty() {
        return Err(CliError::user(
            CMD_REGISTER,
            "--emails must contain at least one address",
        ));
    }
    let machines: Vec<String> = args
        .machines
        .as_deref()
        .map(|s| {
            s.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    let mut capabilities = PartialCapabilityMatrix::default();
    for raw in &args.capabilities {
        let (k, v) = raw.split_once('=').ok_or_else(|| {
            CliError::user(
                CMD_REGISTER,
                format!("capability `{raw}` must be `key=value`"),
            )
        })?;
        let key = k.trim().to_string();
        let value: bool = match v.trim() {
            "true" => true,
            "false" => false,
            other => {
                return Err(CliError::user(
                    CMD_REGISTER,
                    format!("capability `{key}` value must be `true` or `false`, got `{other}`"),
                ));
            }
        };
        match key.as_str() {
            "can_promote_verified" => capabilities.can_promote_verified = Some(value),
            "can_close_high_risk" => capabilities.can_close_high_risk = Some(value),
            "can_force_push" => capabilities.can_force_push = Some(value),
            "can_redact" => capabilities.can_redact = Some(value),
            _ => {
                capabilities.extra.insert(key, value);
            }
        }
    }

    let new = RegisteredIdentity {
        id: args.id.clone(),
        name: args.name.clone(),
        kind: args.kind.to_core(),
        emails,
        machines,
        capabilities,
        status: IdentityStatus::Active,
    };
    let serialized = SerializedIdentity::from(&new);
    registry.identities.push(new);
    registry
        .save(&ws.root)
        .map_err(|e| CliError::internal(CMD_REGISTER, format!("save registry: {e}")))?;

    let _ = global;
    Ok(CommandOutcome::IdentityRegister(IdentityRegisterOutcome {
        command: CMD_REGISTER,
        identity: serialized,
        path: ws.root.join(REGISTRY_FILENAME),
        warnings: Vec::new(),
    }))
}

/// `firetrail identity list`
pub fn list(args: &IdentityListArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let ws = crate::workspace::require_initialised(CMD_LIST, global.workspace.as_deref())?;
    let registry = load_registry(&ws.root)
        .map_err(|e| CliError::internal(CMD_LIST, format!("load registry: {e}")))?;
    let filter = args.status;
    let identities: Vec<SerializedIdentity> = registry
        .identities
        .iter()
        .filter(|i| match filter {
            None => true,
            Some(IdentityStatusArg::Active) => matches!(i.status, IdentityStatus::Active),
            Some(IdentityStatusArg::Offboarded) => matches!(i.status, IdentityStatus::Offboarded),
        })
        .map(SerializedIdentity::from)
        .collect();
    let _ = global;
    Ok(CommandOutcome::IdentityList(IdentityListOutcome {
        command: CMD_LIST,
        identities,
        warnings: Vec::new(),
    }))
}

/// `firetrail identity show`
pub fn show(args: &IdentityShowArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let ws = crate::workspace::require_initialised(CMD_SHOW, global.workspace.as_deref())?;
    let registry = load_registry(&ws.root)
        .map_err(|e| CliError::internal(CMD_SHOW, format!("load registry: {e}")))?;
    let ident = registry
        .identities
        .iter()
        .find(|i| i.id == args.id)
        .ok_or_else(|| CliError::NotFound {
            command: CMD_SHOW.into(),
            what: args.id.clone(),
        })?;
    let _ = global;
    Ok(CommandOutcome::IdentityShow(IdentityShowOutcome {
        command: CMD_SHOW,
        identity: SerializedIdentity::from(ident),
        warnings: Vec::new(),
    }))
}

/// `firetrail identity offboard`
pub fn offboard(
    args: &IdentityOffboardArgs,
    global: &GlobalOpts,
) -> Result<CommandOutcome, CliError> {
    let mut ctx = WorkCtx::open(CMD_OFFBOARD, global.workspace.as_deref())?;
    let mut registry = load_registry(&ctx.ws.root)
        .map_err(|e| CliError::internal(CMD_OFFBOARD, format!("load registry: {e}")))?;
    let aliases: Vec<String> = registry
        .identities
        .iter()
        .find(|i| i.id == args.id)
        .map(|i| i.emails.clone())
        .ok_or_else(|| CliError::NotFound {
            command: CMD_OFFBOARD.into(),
            what: args.id.clone(),
        })?;
    registry
        .offboard(&args.id)
        .map_err(|e| CliError::internal(CMD_OFFBOARD, format!("offboard: {e}")))?;
    registry
        .save(&ctx.ws.root)
        .map_err(|e| CliError::internal(CMD_OFFBOARD, format!("save registry: {e}")))?;

    let mut released: Vec<String> = Vec::new();
    if args.sweep_claims {
        // Walk every record once; gather records claimed by any of the
        // identity's aliases (the canonical id itself may also appear on
        // records if a workspace uses tokens directly).
        let ids = ctx
            .storage
            .list(&StorageFilter::default())
            .map_err(|e| CliError::internal(CMD_OFFBOARD, format!("list storage: {e}")))?;
        let mut targets: Vec<String> = aliases.clone();
        targets.push(args.id.clone());

        let mut records = Vec::with_capacity(ids.len());
        for id in &ids {
            match ctx.storage.read(id) {
                Ok(r) => records.push(r),
                Err(e) => ctx
                    .warnings
                    .push(format!("could not read {id} during sweep: {e}")),
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
                    ctx.warnings
                        .push(format!("could not re-read {rid} for release: {e}"));
                    continue;
                }
            };
            clear_claim(&mut record.body);
            record.envelope.updated_at = chrono::Utc::now();
            match ctx.save_record(&mut record) {
                Ok(_) => released.push(rid.as_str().to_string()),
                Err(e) => ctx
                    .warnings
                    .push(format!("could not release claim on {rid}: {e}")),
            }
        }
    }

    Ok(CommandOutcome::IdentityOffboard(IdentityOffboardOutcome {
        command: CMD_OFFBOARD,
        id: args.id.clone(),
        swept: args.sweep_claims,
        released_claims: released,
        warnings: ctx.warnings.clone(),
    }))
}

fn clear_claim(body: &mut RecordBody) {
    match body {
        RecordBody::Task(t) => t.claim = None,
        RecordBody::Subtask(s) => s.claim = None,
        RecordBody::Bug(b) => b.claim = None,
        _ => {}
    }
}

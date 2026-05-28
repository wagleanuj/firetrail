//! Claim / unclaim ops with lockfile atomicity.
//!
//! Concurrent claim attempts on the same record are serialized via a per-record
//! `O_CREAT|O_EXCL` lockfile under `.firetrail/locks/<id>.claim`. The first
//! writer wins; subsequent attempts get [`OpsError::Conflict`].
//!
//! Takeover semantics live in `unclaim`: passing `takeover: true` lets the
//! caller release another actor's claim when (a) the claim has already expired
//! or (b) the caller holds the admin capability in the identity registry.

use chrono::{Duration, Utc};
use ft_core::{Claim, Record, RecordBody, RecordKind};
use ft_identity::{ClaimInfo, can_take_over, is_claim_expired, load_registry};
use serde::{Deserialize, Serialize};

use crate::error::OpsError;
use crate::events::{Event, EventBus};
use crate::identity::Identity;
use crate::workspace::Workspace;

use super::ctx::{LockHandle, TicketCtx};

/// Default claim duration when [`ClaimInput::expires`] is omitted.
const DEFAULT_CLAIM_DURATION: Duration = Duration::days(7);

/// Input for [`claim`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClaimInput {
    /// Ticket id (full canonical or unambiguous prefix).
    pub id: String,
    /// Human-readable duration override (e.g. `"7d"`, `"12h"`). Defaults to
    /// 7 days when omitted.
    #[serde(default)]
    pub expires: Option<String>,
}

/// Output of [`claim`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClaimOutput {
    /// The updated record with the new claim attached.
    pub record: Record,
}

/// `claim` op — atomically attach a claim to a ticket.
pub fn claim(
    ws: &Workspace,
    identity: &Identity,
    input: ClaimInput,
    events: &EventBus,
) -> Result<ClaimOutput, OpsError> {
    let mut ctx = TicketCtx::open(ws, identity, "claim")?;
    let id = ctx.resolve_id(&input.id)?;
    if !claim_supported(id.kind()) {
        return Err(OpsError::validation(
            "id",
            format!("{:?} records do not support claims", id.kind()),
        ));
    }
    let actor = ctx.actor.clone();
    let duration = match input.expires {
        Some(s) => parse_duration(&s)?,
        None => DEFAULT_CLAIM_DURATION,
    };

    let _lock = LockHandle::acquire(ws, &id, "claim")?;

    let mut record = ctx.read_record(&id)?;
    if let Some(existing) = existing_claim(&record.body) {
        if existing.claim_expires_at > Utc::now() {
            return Err(OpsError::Conflict {
                reason: format!(
                    "{} is already claimed by `{}` until {}",
                    id,
                    existing.claimed_by,
                    existing.claim_expires_at.to_rfc3339()
                ),
            });
        }
    }

    let now = Utc::now();
    let claim = Claim {
        claimed_by: actor.clone(),
        claimed_at: now,
        claim_source: "ops".to_string(),
        claim_expires_at: now + duration,
    };
    set_claim(&mut record.body, Some(claim));
    if record.envelope.owner.is_none() {
        record.envelope.owner = Some(actor.clone());
    }
    record.envelope.updated_at = now;
    ctx.save_record(&mut record)?;

    events.emit(Event::TicketClaimed {
        id: record.envelope.id.as_str().to_string(),
        actor: actor.as_str().to_string(),
    });

    Ok(ClaimOutput { record })
}

/// Input for [`unclaim`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UnclaimInput {
    /// Ticket id.
    pub id: String,
    /// Release another actor's claim. When `true`, the caller must either be
    /// the existing claimant, the existing claim must be expired, or the
    /// caller must hold the admin capability.
    #[serde(default)]
    pub takeover: bool,
    /// Required when `takeover` is `true`.
    #[serde(default)]
    pub reason: Option<String>,
}

/// Output of [`unclaim`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UnclaimOutput {
    /// The updated record.
    pub record: Record,
}

/// `unclaim` op — release the active claim on a ticket.
#[allow(clippy::needless_pass_by_value)]
pub fn unclaim(
    ws: &Workspace,
    identity: &Identity,
    input: UnclaimInput,
    events: &EventBus,
) -> Result<UnclaimOutput, OpsError> {
    if input.takeover && input.reason.is_none() {
        return Err(OpsError::validation(
            "reason",
            "--takeover requires --reason",
        ));
    }

    let mut ctx = TicketCtx::open(ws, identity, "unclaim")?;
    let id = ctx.resolve_id(&input.id)?;
    let actor = ctx.actor.clone();
    let mut record = ctx.read_record(&id)?;

    let current = existing_claim(&record.body).cloned();
    let Some(c) = current else {
        return Err(OpsError::Conflict {
            reason: "record has no active claim".to_string(),
        });
    };
    if c.claimed_by != actor {
        if !input.takeover {
            return Err(OpsError::Conflict {
                reason: format!(
                    "claim is held by `{}`; pass takeover + reason to release another actor's claim",
                    c.claimed_by
                ),
            });
        }
        let info = ClaimInfo {
            actor: c.claimed_by.clone(),
            claim_expires_at: c.claim_expires_at,
            on_behalf_of: None,
        };
        let now = Utc::now();
        if !is_claim_expired(&info, now) {
            let registry = load_registry(&ws.root)
                .map_err(|e| OpsError::Internal(anyhow::anyhow!("load registry: {e}")))?;
            if !can_take_over(&info, &actor, &registry, now) {
                return Err(OpsError::PermissionDenied {
                    reason: format!(
                        "claim on {id} is live (held by `{}`); takeover requires the admin capability when the claim has not expired",
                        c.claimed_by
                    ),
                });
            }
        }
    }
    set_claim(&mut record.body, None);
    record.envelope.updated_at = Utc::now();
    ctx.save_record(&mut record)?;

    // Best-effort lockfile cleanup.
    let lower = id.as_str().to_lowercase();
    let _ = std::fs::remove_file(
        ws.firetrail_dir()
            .join("locks")
            .join(format!("{lower}.claim")),
    );

    events.emit(Event::TicketUnclaimed {
        id: record.envelope.id.as_str().to_string(),
    });

    Ok(UnclaimOutput { record })
}

fn claim_supported(kind: RecordKind) -> bool {
    matches!(
        kind,
        RecordKind::Task | RecordKind::Subtask | RecordKind::Bug
    )
}

fn existing_claim(body: &RecordBody) -> Option<&Claim> {
    match body {
        RecordBody::Task(t) => t.claim.as_ref(),
        RecordBody::Subtask(s) => s.claim.as_ref(),
        RecordBody::Bug(b) => b.claim.as_ref(),
        _ => None,
    }
}

fn set_claim(body: &mut RecordBody, claim: Option<Claim>) {
    match body {
        RecordBody::Task(t) => t.claim = claim,
        RecordBody::Subtask(s) => s.claim = claim,
        RecordBody::Bug(b) => b.claim = claim,
        _ => {}
    }
}

fn parse_duration(s: &str) -> Result<Duration, OpsError> {
    let raw = humantime::parse_duration(s).map_err(|e| {
        OpsError::validation(
            "expires",
            format!("invalid duration `{s}`: {e} (try `7d`, `12h`, `30m`)"),
        )
    })?;
    let secs = i64::try_from(raw.as_secs())
        .map_err(|_| OpsError::validation("expires", format!("duration `{s}` is too large")))?;
    Ok(Duration::seconds(secs))
}

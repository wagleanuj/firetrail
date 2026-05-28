//! `firetrail claim <id>` / `firetrail unclaim <id>`.
//!
//! ## Atomicity strategy
//!
//! Concurrent claim attempts on the same record must produce exactly one
//! winner. We rely on POSIX `O_CREAT|O_EXCL` semantics by creating a
//! per-record lockfile under `.firetrail/locks/<lower-id>.claim` via
//! `OpenOptions::new().create_new(true)`. The first writer wins; subsequent
//! attempts get `AlreadyExists`, which we map to [`CliError::Conflict`] with a
//! useful message.
//!
//! The lockfile is *also* removed once the claim is recorded into the record's
//! body; failure to remove is non-fatal — the next claim attempt will still
//! see the on-record `Claim` (which is the source of truth) and refuse.

use std::fs::{self, OpenOptions};
use std::io::ErrorKind;
use std::path::PathBuf;

use chrono::{Duration, Utc};
use ft_core::{Claim, RecordBody, RecordKind};

use crate::cli::{ClaimArgs, ClaimTakeoverArgs, GlobalOpts, UnclaimArgs};
use crate::commands::{CommandOutcome, RecordOutcome};
use crate::context::WorkCtx;
use crate::error::CliError;
use crate::workspace::Workspace;

const COMMAND_CLAIM: &str = "claim";
const COMMAND_UNCLAIM: &str = "unclaim";
const COMMAND_TAKEOVER: &str = "claim-takeover";

/// Default claim duration when neither `--expires` nor workspace config
/// overrides it.
const DEFAULT_CLAIM_DURATION: Duration = Duration::days(7);

/// `firetrail claim`
pub fn claim(args: &ClaimArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let mut ctx = WorkCtx::open(COMMAND_CLAIM, global.workspace.as_deref())?;
    let id = ctx.resolve_id(&args.id)?;
    if !claim_supported(id.kind()) {
        return Err(CliError::user(
            COMMAND_CLAIM,
            format!("{:?} records do not support claims", id.kind()),
        ));
    }

    let actor = ctx.actor()?;
    let duration = match &args.expires {
        Some(s) => parse_duration(s)?,
        None => DEFAULT_CLAIM_DURATION,
    };

    // Acquire lockfile.
    let lock_path = lock_path(&ctx.ws, &id);
    let _lock = LockHandle::acquire(&lock_path)?;

    let mut record = ctx.read_record(&id)?;

    // Re-check the record body for an existing live claim now that we hold
    // the lock — guards against stale lockfile cleanup races.
    if let Some(existing) = existing_claim(&record.body) {
        if existing.claim_expires_at > Utc::now() {
            return Err(CliError::Conflict {
                command: COMMAND_CLAIM.into(),
                message: format!(
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
        claim_source: "cli".to_string(),
        claim_expires_at: now + duration,
    };
    set_claim(&mut record.body, Some(claim));
    if record.envelope.owner.is_none() {
        record.envelope.owner = Some(actor);
    }
    record.envelope.updated_at = now;
    ctx.save_record(&mut record)?;

    Ok(CommandOutcome::Claimed(
        RecordOutcome::new(COMMAND_CLAIM, record).with_warnings(ctx.warnings.clone()),
    ))
}

/// `firetrail unclaim`
pub fn unclaim(args: &UnclaimArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    use ft_identity::{ClaimInfo, can_take_over, is_claim_expired, load_registry};

    if args.takeover && args.reason.is_none() {
        return Err(CliError::user(
            COMMAND_UNCLAIM,
            "--takeover requires --reason",
        ));
    }

    let mut ctx = WorkCtx::open(COMMAND_UNCLAIM, global.workspace.as_deref())?;
    let id = ctx.resolve_id(&args.id)?;
    let actor = ctx.actor()?;
    let mut record = ctx.read_record(&id)?;

    let current = existing_claim(&record.body).cloned();
    let Some(c) = current else {
        return Err(CliError::user(
            COMMAND_UNCLAIM,
            "record has no active claim",
        ));
    };
    if c.claimed_by != actor {
        if !args.takeover {
            return Err(CliError::Conflict {
                command: COMMAND_UNCLAIM.into(),
                message: format!(
                    "claim is held by `{}`; use --takeover --reason to release another actor's claim",
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
            let registry = load_registry(&ctx.ws.root)
                .map_err(|e| CliError::internal(COMMAND_UNCLAIM, format!("load registry: {e}")))?;
            if !can_take_over(&info, &actor, &registry, now) {
                return Err(CliError::Conflict {
                    command: COMMAND_UNCLAIM.into(),
                    message: format!(
                        "claim on {id} is live (held by `{}`); --takeover requires the admin capability when the claim has not expired",
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
    let _ = fs::remove_file(lock_path(&ctx.ws, &id));

    Ok(CommandOutcome::Updated(
        RecordOutcome::new(COMMAND_UNCLAIM, record).with_warnings(ctx.warnings.clone()),
    ))
}

/// `firetrail claim-takeover` (M5).
///
/// Rewrites the active claim atomically. Two policy gates:
///
/// - Default: only succeeds when the existing claim is expired (anyone may
///   pick up an abandoned claim).
/// - With `--force`: also succeeds when the caller has the `can_force_push`
///   admin capability in the identity registry. The check is delegated to
///   [`ft_identity::can_take_over`].
pub fn takeover(args: &ClaimTakeoverArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    use ft_identity::{ClaimInfo, can_take_over, is_claim_expired, load_registry};

    let mut ctx = WorkCtx::open(COMMAND_TAKEOVER, global.workspace.as_deref())?;
    let id = ctx.resolve_id(&args.id)?;
    if !claim_supported(id.kind()) {
        return Err(CliError::user(
            COMMAND_TAKEOVER,
            format!("{:?} records do not support claims", id.kind()),
        ));
    }
    let actor = ctx.actor()?;
    let registry = load_registry(&ctx.ws.root)
        .map_err(|e| CliError::internal(COMMAND_TAKEOVER, format!("load registry: {e}")))?;

    let lock_path = lock_path(&ctx.ws, &id);
    let _lock = LockHandle::acquire_for(&lock_path, COMMAND_TAKEOVER)?;

    let mut record = ctx.read_record(&id)?;
    let existing = existing_claim(&record.body).cloned().ok_or_else(|| {
        CliError::user(COMMAND_TAKEOVER, "record has no active claim to take over")
    })?;

    let info = ClaimInfo {
        actor: existing.claimed_by.clone(),
        claim_expires_at: existing.claim_expires_at,
        on_behalf_of: None,
    };
    let now = Utc::now();
    let expired = is_claim_expired(&info, now);
    if !expired {
        if !args.force {
            return Err(CliError::Conflict {
                command: COMMAND_TAKEOVER.into(),
                message: format!(
                    "claim on {id} is live (held by `{}` until {}); pass --force when you hold admin capability",
                    existing.claimed_by,
                    existing.claim_expires_at.to_rfc3339()
                ),
            });
        }
        if !can_take_over(&info, &actor, &registry, now) {
            return Err(CliError::Conflict {
                command: COMMAND_TAKEOVER.into(),
                message: format!(
                    "caller `{actor}` lacks the admin capability to force-take a live claim"
                ),
            });
        }
    }

    let new_claim = Claim {
        claimed_by: actor.clone(),
        claimed_at: now,
        claim_source: "cli-takeover".to_string(),
        claim_expires_at: now + DEFAULT_CLAIM_DURATION,
    };
    set_claim(&mut record.body, Some(new_claim));
    record.envelope.owner = Some(actor);
    record.envelope.updated_at = now;
    ctx.save_record(&mut record)?;

    Ok(CommandOutcome::Claimed(
        RecordOutcome::new(COMMAND_TAKEOVER, record).with_warnings(ctx.warnings.clone()),
    ))
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

fn lock_path(ws: &Workspace, id: &ft_core::RecordId) -> PathBuf {
    let lower = id.as_str().to_lowercase();
    ws.firetrail_dir()
        .join("locks")
        .join(format!("{lower}.claim"))
}

/// RAII lockfile handle. Dropping it removes the file (best effort).
struct LockHandle {
    path: PathBuf,
}

impl LockHandle {
    fn acquire(path: &PathBuf) -> Result<Self, CliError> {
        Self::acquire_for(path, COMMAND_CLAIM)
    }

    fn acquire_for(path: &PathBuf, command: &'static str) -> Result<Self, CliError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| CliError::internal(command, format!("locks dir: {e}")))?;
        }
        match OpenOptions::new().write(true).create_new(true).open(path) {
            Ok(_f) => Ok(Self { path: path.clone() }),
            Err(e) if e.kind() == ErrorKind::AlreadyExists => Err(CliError::Conflict {
                command: command.into(),
                message: "another claim is in-flight for this record".into(),
            }),
            Err(e) => Err(CliError::internal(command, format!("lockfile error: {e}"))),
        }
    }
}

impl Drop for LockHandle {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn parse_duration(s: &str) -> Result<Duration, CliError> {
    let raw = humantime::parse_duration(s).map_err(|e| {
        CliError::user(
            COMMAND_CLAIM,
            format!("invalid duration `{s}`: {e} (try `7d`, `12h`, `30m`)"),
        )
    })?;
    let secs = i64::try_from(raw.as_secs())
        .map_err(|_| CliError::user(COMMAND_CLAIM, format!("duration `{s}` is too large")))?;
    Ok(Duration::seconds(secs))
}

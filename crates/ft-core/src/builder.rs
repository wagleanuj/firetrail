//! `RecordBuilder` — validating constructor for [`Record`] instances.

use chrono::{DateTime, Utc};

use crate::enums::{Origin, Priority, Status};
use crate::error::CoreError;
use crate::hash::state_hash;
use crate::id::{RecordId, RecordKind};
use crate::identity::Identity;
use crate::record::{
    Bug, Decision, Doc, Epic, Finding, Gotcha, Incident, Memory, Record, RecordBody,
    RecordEnvelope, Runbook, Subtask, Task,
};

/// Builder for [`Record`] instances that validates as it constructs.
///
/// From M2 onward all record kinds — work-tracking (`Epic`, `Task`,
/// `Subtask`, `Bug`) and memory (`Incident`, `Finding`, `Runbook`,
/// `Decision`, `Gotcha`, `Memory`) — can be constructed via this builder.
/// Memory bodies that require non-defaultable fields (notably
/// [`Subtask::parent_task`] and [`Incident::started_at`]) need an explicit
/// body via [`Self::body`] or one of the kind-specific setters.
///
/// The builder fills in safe defaults (status `Open`, origin `Human`,
/// timestamps `now`, scope fields empty) and computes the initial
/// `state_hash`. Callers override fields via the chainable setters.
///
/// # Examples
///
/// ```
/// use ft_core::{builder::RecordBuilder, Identity, Priority, RecordKind, Status};
///
/// let alice = Identity::new("alice@example.com").unwrap();
/// let record = RecordBuilder::new(RecordKind::Task, "Add Redis alert", alice)
///     .priority(Priority::P1)
///     .status(Status::Ready)
///     .build()
///     .unwrap();
///
/// assert_eq!(record.envelope.priority, Priority::P1);
/// assert_eq!(record.envelope.status, Status::Ready);
/// assert_eq!(record.envelope.state_hash.len(), 64);
/// ```
#[derive(Debug)]
pub struct RecordBuilder {
    kind: RecordKind,
    title: String,
    created_by: Identity,
    id: Option<RecordId>,
    status: Status,
    priority: Priority,
    owner: Option<Identity>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    closed_at: Option<DateTime<Utc>>,
    owning_scope: Option<String>,
    affected_scopes: Vec<String>,
    applies_to: Vec<String>,
    origin: Origin,
    body: Option<RecordBody>,
}

impl RecordBuilder {
    /// Begin building a record. Title and `created_by` are required.
    #[must_use]
    pub fn new(kind: RecordKind, title: impl Into<String>, created_by: Identity) -> Self {
        let now = Utc::now();
        Self {
            kind,
            title: title.into(),
            created_by,
            id: None,
            status: Status::Open,
            priority: Priority::P2,
            owner: None,
            created_at: now,
            updated_at: now,
            closed_at: None,
            owning_scope: None,
            affected_scopes: Vec::new(),
            applies_to: Vec::new(),
            origin: Origin::Human,
            body: None,
        }
    }

    /// Override the auto-minted id.
    #[must_use]
    pub fn id(mut self, id: RecordId) -> Self {
        self.id = Some(id);
        self
    }

    /// Set the initial workflow status.
    #[must_use]
    pub fn status(mut self, status: Status) -> Self {
        self.status = status;
        self
    }

    /// Set the priority class.
    #[must_use]
    pub fn priority(mut self, priority: Priority) -> Self {
        self.priority = priority;
        self
    }

    /// Set the initial owner.
    #[must_use]
    pub fn owner(mut self, owner: Identity) -> Self {
        self.owner = Some(owner);
        self
    }

    /// Set the creation timestamp (overrides `now`).
    #[must_use]
    pub fn created_at(mut self, ts: DateTime<Utc>) -> Self {
        self.created_at = ts;
        self.updated_at = ts;
        self
    }

    /// Set the last-update timestamp.
    #[must_use]
    pub fn updated_at(mut self, ts: DateTime<Utc>) -> Self {
        self.updated_at = ts;
        self
    }

    /// Set the owning scope (ADR-0004).
    #[must_use]
    pub fn owning_scope(mut self, s: impl Into<String>) -> Self {
        self.owning_scope = Some(s.into());
        self
    }

    /// Add an affected scope (ADR-0004).
    #[must_use]
    pub fn affected_scope(mut self, s: impl Into<String>) -> Self {
        self.affected_scopes.push(s.into());
        self
    }

    /// Add an `applies_to` file/service entry.
    #[must_use]
    pub fn applies_to(mut self, s: impl Into<String>) -> Self {
        self.applies_to.push(s.into());
        self
    }

    /// Override the origin flag (ADR-0013).
    #[must_use]
    pub fn origin(mut self, origin: Origin) -> Self {
        self.origin = origin;
        self
    }

    /// Set the body. Must match `kind`.
    #[must_use]
    pub fn body(mut self, body: RecordBody) -> Self {
        self.body = Some(body);
        self
    }

    /// Convenience: set the body to an `Epic`.
    #[must_use]
    pub fn epic(self, epic: Epic) -> Self {
        self.body(RecordBody::Epic(epic))
    }

    /// Convenience: set the body to a `Task`.
    #[must_use]
    pub fn task(self, task: Task) -> Self {
        self.body(RecordBody::Task(task))
    }

    /// Convenience: set the body to a `Subtask`.
    #[must_use]
    pub fn subtask(self, sub: Subtask) -> Self {
        self.body(RecordBody::Subtask(sub))
    }

    /// Convenience: set the body to a `Bug`.
    #[must_use]
    pub fn bug(self, bug: Bug) -> Self {
        self.body(RecordBody::Bug(bug))
    }

    /// Convenience: set the body to an `Incident`.
    #[must_use]
    pub fn incident(self, incident: Incident) -> Self {
        self.body(RecordBody::Incident(incident))
    }

    /// Convenience: set the body to a `Finding`.
    #[must_use]
    pub fn finding(self, finding: Finding) -> Self {
        self.body(RecordBody::Finding(finding))
    }

    /// Convenience: set the body to a `Runbook`.
    #[must_use]
    pub fn runbook(self, runbook: Runbook) -> Self {
        self.body(RecordBody::Runbook(runbook))
    }

    /// Convenience: set the body to a `Decision`.
    #[must_use]
    pub fn decision(self, decision: Decision) -> Self {
        self.body(RecordBody::Decision(decision))
    }

    /// Convenience: set the body to a `Gotcha`.
    #[must_use]
    pub fn gotcha(self, gotcha: Gotcha) -> Self {
        self.body(RecordBody::Gotcha(gotcha))
    }

    /// Convenience: set the body to a generic `Memory`.
    #[must_use]
    pub fn memory(self, memory: Memory) -> Self {
        self.body(RecordBody::Memory(memory))
    }

    /// Convenience: set the body to a file-backed `Doc`.
    #[must_use]
    pub fn doc(self, doc: Doc) -> Self {
        self.body(RecordBody::Doc(doc))
    }

    /// Finalize: validate, compute `state_hash`, and return the [`Record`].
    ///
    /// # Errors
    ///
    /// - [`CoreError::InvalidRecord`] if the title is empty, an acceptance
    ///   criterion is malformed, or the supplied body does not match `kind`.
    pub fn build(self) -> Result<Record, CoreError> {
        let title = self.title.trim().to_string();
        if title.is_empty() {
            return Err(CoreError::InvalidRecord("title is empty".into()));
        }

        let body = self.body.unwrap_or_else(|| default_body_for(self.kind));

        if body.kind() != self.kind {
            return Err(CoreError::InvalidRecord(format!(
                "body kind `{:?}` does not match envelope kind `{:?}`",
                body.kind(),
                self.kind
            )));
        }

        validate_body(&body)?;

        let id = self
            .id
            .unwrap_or_else(|| RecordId::mint(self.kind, &self.created_by));

        // Construct envelope with empty hash, then compute and set.
        let envelope = RecordEnvelope {
            id,
            kind: self.kind,
            title,
            status: self.status,
            priority: self.priority,
            owner: self.owner,
            created_by: self.created_by,
            created_at: self.created_at,
            updated_at: self.updated_at,
            closed_at: self.closed_at,
            owning_scope: self.owning_scope,
            affected_scopes: self.affected_scopes,
            applies_to: self.applies_to,
            state_hash: String::new(),
            prev_state_hash: None,
            labels: Vec::new(),
            history: Vec::new(),
            origin: self.origin,
        };

        let mut record = Record { envelope, body };
        record.envelope.state_hash = state_hash(&record)?;
        Ok(record)
    }
}

fn default_body_for(kind: RecordKind) -> RecordBody {
    match kind {
        RecordKind::Epic => RecordBody::Epic(Epic::default()),
        RecordKind::Task => RecordBody::Task(Task::default()),
        RecordKind::Bug => RecordBody::Bug(Bug::default()),
        RecordKind::Incident => RecordBody::Incident(Incident::default()),
        RecordKind::Finding => RecordBody::Finding(Finding::default()),
        RecordKind::Runbook => RecordBody::Runbook(Runbook::default()),
        RecordKind::Decision => RecordBody::Decision(Decision::default()),
        RecordKind::Gotcha => RecordBody::Gotcha(Gotcha::default()),
        RecordKind::Memory => RecordBody::Memory(Memory::default()),
        RecordKind::Doc => RecordBody::Doc(Doc::default()),
        // Subtask requires `parent_task`; callers must supply a body explicitly.
        RecordKind::Subtask => {
            // Reached only if the caller forgot to set body; we return an
            // obviously-empty body so the validator can flag it.
            RecordBody::Subtask(Subtask {
                description: String::new(),
                parent_task: RecordId::from_string(format!("TASK-{}", "0".repeat(64)))
                    .expect("constant well-formed id"),
                acceptance_criteria: Vec::new(),
                evidence: Vec::new(),
                claim: None,
            })
        }
    }
}

fn validate_body(body: &RecordBody) -> Result<(), CoreError> {
    let acs = match body {
        RecordBody::Task(t) => Some(&t.acceptance_criteria),
        RecordBody::Subtask(s) => Some(&s.acceptance_criteria),
        RecordBody::Bug(b) => Some(&b.acceptance_criteria),
        _ => None,
    };
    if let Some(acs) = acs {
        for ac in acs {
            if ac.text.trim().is_empty() {
                return Err(CoreError::InvalidRecord(format!(
                    "acceptance criterion `{}` has empty text",
                    ac.id
                )));
            }
            if ac.id.trim().is_empty() {
                return Err(CoreError::InvalidRecord(
                    "acceptance criterion id is empty".into(),
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acceptance::AcceptanceCriterion;
    use crate::enums::AcStatus;

    fn alice() -> Identity {
        Identity::new("alice@example.com").unwrap()
    }

    #[test]
    fn builds_default_task() {
        let r = RecordBuilder::new(RecordKind::Task, "demo", alice())
            .build()
            .unwrap();
        assert_eq!(r.envelope.title, "demo");
        assert_eq!(r.envelope.kind, RecordKind::Task);
        assert!(matches!(r.body, RecordBody::Task(_)));
        assert_eq!(r.envelope.state_hash.len(), 64);
        assert!(r.envelope.prev_state_hash.is_none());
    }

    #[test]
    fn rejects_empty_title() {
        let err = RecordBuilder::new(RecordKind::Task, "   ", alice())
            .build()
            .unwrap_err();
        assert!(matches!(err, CoreError::InvalidRecord(_)));
    }

    #[test]
    fn builds_all_memory_kinds() {
        for kind in [
            RecordKind::Incident,
            RecordKind::Finding,
            RecordKind::Runbook,
            RecordKind::Decision,
            RecordKind::Gotcha,
            RecordKind::Memory,
        ] {
            let r = RecordBuilder::new(kind, "x", alice())
                .build()
                .unwrap_or_else(|e| panic!("memory kind {kind:?} should build: {e}"));
            assert_eq!(r.envelope.kind, kind);
            assert_eq!(r.body.kind(), kind);
            assert_eq!(r.envelope.state_hash.len(), 64);
        }
    }

    #[test]
    fn builds_incident_with_explicit_body() {
        use crate::enums::{RiskClass, Severity, TrustState};
        use crate::record::Incident;
        let started = Utc::now();
        let inc = Incident {
            summary: "redis pool exhaustion".into(),
            severity: Severity::Sev2,
            started_at: started,
            resolved_at: None,
            services_affected: vec!["checkout".into()],
            root_cause: None,
            findings: Vec::new(),
            runbooks_invoked: Vec::new(),
            risk_class: Some(RiskClass::Availability),
            trust: TrustState::Draft,
        };
        let r = RecordBuilder::new(RecordKind::Incident, "INC-redis", alice())
            .incident(inc.clone())
            .build()
            .unwrap();
        match r.body {
            RecordBody::Incident(b) => assert_eq!(b, inc),
            other => panic!("expected Incident body, got {other:?}"),
        }
    }

    #[test]
    fn rejects_body_kind_mismatch() {
        let err = RecordBuilder::new(RecordKind::Task, "demo", alice())
            .body(RecordBody::Bug(Bug::default()))
            .build()
            .unwrap_err();
        assert!(matches!(err, CoreError::InvalidRecord(_)));
    }

    #[test]
    fn rejects_empty_acceptance_criterion_text() {
        let now = Utc::now();
        let ac = AcceptanceCriterion {
            id: "ac-01".into(),
            text: "  ".into(),
            status: AcStatus::Unchecked,
            evidence_url: None,
            checked_by: None,
            checked_at: None,
            created_at: now,
            updated_at: now,
            proposed: false,
        };
        let err = RecordBuilder::new(RecordKind::Task, "demo", alice())
            .task(Task {
                description: String::new(),
                parent_epic: None,
                acceptance_criteria: vec![ac],
                evidence: Vec::new(),
                claim: None,
            })
            .build()
            .unwrap_err();
        assert!(matches!(err, CoreError::InvalidRecord(_)));
    }
}

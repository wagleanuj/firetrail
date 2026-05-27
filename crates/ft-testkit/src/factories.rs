//! Builder-style record factories.
//!
//! Each `make_*()` returns a builder seeded with sensible defaults; chain
//! setters override and call `build()` to obtain a real `ft-core` [`Record`].
//! `build()` panics on validation failure — test callers want to know
//! immediately if a factory produced invalid data.

use chrono::Utc;
use ft_core::{
    AcStatus, AcceptanceCriterion, Bug, Epic, Identity, Label, Priority, Record, RecordBuilder,
    RecordId, RecordKind, Status, Subtask, Task,
};

/// Construct the canonical test identity (deterministic across calls).
#[must_use]
pub fn make_identity() -> Identity {
    Identity::new("tester@firetrail.test").expect("constant identity is valid")
}

/// Construct a named test identity, normalizing whitespace to `_`.
#[must_use]
pub fn make_identity_named(name: &str) -> Identity {
    let sanitized: String = name
        .chars()
        .map(|c| if c.is_whitespace() { '_' } else { c })
        .collect();
    let raw = if sanitized.contains('@') {
        sanitized
    } else {
        format!("{sanitized}@firetrail.test")
    };
    Identity::new(raw).expect("sanitized identity is valid")
}

/// Begin a [`TaskBuilder`].
#[must_use]
pub fn make_task() -> TaskBuilder {
    TaskBuilder::default()
}

/// Begin an [`EpicBuilder`].
#[must_use]
pub fn make_epic() -> EpicBuilder {
    EpicBuilder::default()
}

/// Begin a [`SubtaskBuilder`] with a required parent task id.
#[must_use]
pub fn make_subtask(parent: RecordId) -> SubtaskBuilder {
    SubtaskBuilder::new(parent)
}

/// Begin a [`BugBuilder`].
#[must_use]
pub fn make_bug() -> BugBuilder {
    BugBuilder::default()
}

// ---------------------------------------------------------------------------
// TaskBuilder
// ---------------------------------------------------------------------------

/// Fluent builder for a Task [`Record`].
#[derive(Debug, Clone)]
pub struct TaskBuilder {
    title: String,
    description: String,
    status: Status,
    priority: Priority,
    owner: Option<Identity>,
    created_by: Identity,
    parent_epic: Option<RecordId>,
    acceptance_criteria: Vec<AcceptanceCriterion>,
    labels: Vec<Label>,
    owning_scope: Option<String>,
}

impl Default for TaskBuilder {
    fn default() -> Self {
        Self {
            title: "test task".to_string(),
            description: String::new(),
            status: Status::Open,
            priority: Priority::P2,
            owner: None,
            created_by: make_identity(),
            parent_epic: None,
            acceptance_criteria: Vec::new(),
            labels: Vec::new(),
            owning_scope: None,
        }
    }
}

impl TaskBuilder {
    /// Set the title.
    #[must_use]
    pub fn title(mut self, t: impl Into<String>) -> Self {
        self.title = t.into();
        self
    }
    /// Set the description.
    #[must_use]
    pub fn description(mut self, d: impl Into<String>) -> Self {
        self.description = d.into();
        self
    }
    /// Set the status.
    #[must_use]
    pub fn status(mut self, s: Status) -> Self {
        self.status = s;
        self
    }
    /// Set the priority.
    #[must_use]
    pub fn priority(mut self, p: Priority) -> Self {
        self.priority = p;
        self
    }
    /// Set the owner.
    #[must_use]
    pub fn owner(mut self, o: Identity) -> Self {
        self.owner = Some(o);
        self
    }
    /// Override the creator identity (default: [`make_identity`]).
    #[must_use]
    pub fn created_by(mut self, c: Identity) -> Self {
        self.created_by = c;
        self
    }
    /// Set the parent epic.
    #[must_use]
    pub fn parent_epic(mut self, id: RecordId) -> Self {
        self.parent_epic = Some(id);
        self
    }
    /// Append an acceptance criterion (id auto-assigned as `ac-NN`).
    #[must_use]
    pub fn acceptance_criterion(mut self, text: impl Into<String>) -> Self {
        self.acceptance_criteria
            .push(new_ac(self.acceptance_criteria.len() + 1, text));
        self
    }
    /// Append a free-form label.
    #[must_use]
    pub fn label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.push(Label {
            key: key.into(),
            value: value.into(),
        });
        self
    }
    /// Set the owning scope.
    #[must_use]
    pub fn owning_scope(mut self, scope: impl Into<String>) -> Self {
        self.owning_scope = Some(scope.into());
        self
    }

    /// Finalize into a validated [`Record`].
    ///
    /// # Panics
    ///
    /// Panics if `ft-core` rejects the constructed record.
    #[must_use]
    pub fn build(self) -> Record {
        let body = Task {
            description: self.description,
            parent_epic: self.parent_epic,
            acceptance_criteria: self.acceptance_criteria,
            evidence: Vec::new(),
            claim: None,
        };

        let mut b = RecordBuilder::new(RecordKind::Task, self.title, self.created_by)
            .status(self.status)
            .priority(self.priority)
            .task(body);
        if let Some(o) = self.owner {
            b = b.owner(o);
        }
        if let Some(s) = self.owning_scope {
            b = b.owning_scope(s);
        }
        let mut record = b.build().expect("TaskBuilder produced invalid Record");
        record.envelope.labels = self.labels;
        // Re-hash since labels are part of state.
        record.envelope.state_hash = String::new();
        record.envelope.state_hash =
            ft_core::state_hash(&record).expect("rehash after label append");
        record
    }
}

// ---------------------------------------------------------------------------
// EpicBuilder
// ---------------------------------------------------------------------------

/// Fluent builder for an Epic [`Record`].
#[derive(Debug, Clone)]
pub struct EpicBuilder {
    title: String,
    description: String,
    status: Status,
    priority: Priority,
    owner: Option<Identity>,
    created_by: Identity,
    labels: Vec<Label>,
    owning_scope: Option<String>,
}

impl Default for EpicBuilder {
    fn default() -> Self {
        Self {
            title: "test epic".to_string(),
            description: String::new(),
            status: Status::Open,
            priority: Priority::P2,
            owner: None,
            created_by: make_identity(),
            labels: Vec::new(),
            owning_scope: None,
        }
    }
}

impl EpicBuilder {
    /// Set the title.
    #[must_use]
    pub fn title(mut self, t: impl Into<String>) -> Self {
        self.title = t.into();
        self
    }
    /// Set the description.
    #[must_use]
    pub fn description(mut self, d: impl Into<String>) -> Self {
        self.description = d.into();
        self
    }
    /// Set the status.
    #[must_use]
    pub fn status(mut self, s: Status) -> Self {
        self.status = s;
        self
    }
    /// Set the priority.
    #[must_use]
    pub fn priority(mut self, p: Priority) -> Self {
        self.priority = p;
        self
    }
    /// Set the owner.
    #[must_use]
    pub fn owner(mut self, o: Identity) -> Self {
        self.owner = Some(o);
        self
    }
    /// Override the creator identity.
    #[must_use]
    pub fn created_by(mut self, c: Identity) -> Self {
        self.created_by = c;
        self
    }
    /// Append a free-form label.
    #[must_use]
    pub fn label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.push(Label {
            key: key.into(),
            value: value.into(),
        });
        self
    }
    /// Set the owning scope.
    #[must_use]
    pub fn owning_scope(mut self, scope: impl Into<String>) -> Self {
        self.owning_scope = Some(scope.into());
        self
    }

    /// Finalize into a validated [`Record`].
    #[must_use]
    pub fn build(self) -> Record {
        let body = Epic {
            description: self.description,
            child_ids: Vec::new(),
        };

        let mut b = RecordBuilder::new(RecordKind::Epic, self.title, self.created_by)
            .status(self.status)
            .priority(self.priority)
            .epic(body);
        if let Some(o) = self.owner {
            b = b.owner(o);
        }
        if let Some(s) = self.owning_scope {
            b = b.owning_scope(s);
        }
        let mut record = b.build().expect("EpicBuilder produced invalid Record");
        record.envelope.labels = self.labels;
        record.envelope.state_hash = String::new();
        record.envelope.state_hash =
            ft_core::state_hash(&record).expect("rehash after label append");
        record
    }
}

// ---------------------------------------------------------------------------
// SubtaskBuilder
// ---------------------------------------------------------------------------

/// Fluent builder for a Subtask [`Record`].
#[derive(Debug, Clone)]
pub struct SubtaskBuilder {
    title: String,
    description: String,
    status: Status,
    priority: Priority,
    owner: Option<Identity>,
    created_by: Identity,
    parent_task: RecordId,
    acceptance_criteria: Vec<AcceptanceCriterion>,
    labels: Vec<Label>,
    owning_scope: Option<String>,
}

impl SubtaskBuilder {
    /// Construct from a required parent task id.
    #[must_use]
    pub fn new(parent: RecordId) -> Self {
        Self {
            title: "test subtask".to_string(),
            description: String::new(),
            status: Status::Open,
            priority: Priority::P2,
            owner: None,
            created_by: make_identity(),
            parent_task: parent,
            acceptance_criteria: Vec::new(),
            labels: Vec::new(),
            owning_scope: None,
        }
    }

    /// Set the title.
    #[must_use]
    pub fn title(mut self, t: impl Into<String>) -> Self {
        self.title = t.into();
        self
    }
    /// Set the description.
    #[must_use]
    pub fn description(mut self, d: impl Into<String>) -> Self {
        self.description = d.into();
        self
    }
    /// Set the status.
    #[must_use]
    pub fn status(mut self, s: Status) -> Self {
        self.status = s;
        self
    }
    /// Set the priority.
    #[must_use]
    pub fn priority(mut self, p: Priority) -> Self {
        self.priority = p;
        self
    }
    /// Set the owner.
    #[must_use]
    pub fn owner(mut self, o: Identity) -> Self {
        self.owner = Some(o);
        self
    }
    /// Override the creator identity.
    #[must_use]
    pub fn created_by(mut self, c: Identity) -> Self {
        self.created_by = c;
        self
    }
    /// Append an acceptance criterion (id auto-assigned as `ac-NN`).
    #[must_use]
    pub fn acceptance_criterion(mut self, text: impl Into<String>) -> Self {
        self.acceptance_criteria
            .push(new_ac(self.acceptance_criteria.len() + 1, text));
        self
    }
    /// Append a free-form label.
    #[must_use]
    pub fn label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.push(Label {
            key: key.into(),
            value: value.into(),
        });
        self
    }
    /// Set the owning scope.
    #[must_use]
    pub fn owning_scope(mut self, scope: impl Into<String>) -> Self {
        self.owning_scope = Some(scope.into());
        self
    }

    /// Finalize into a validated [`Record`].
    #[must_use]
    pub fn build(self) -> Record {
        let body = Subtask {
            description: self.description,
            parent_task: self.parent_task,
            acceptance_criteria: self.acceptance_criteria,
            evidence: Vec::new(),
            claim: None,
        };

        let mut b = RecordBuilder::new(RecordKind::Subtask, self.title, self.created_by)
            .status(self.status)
            .priority(self.priority)
            .subtask(body);
        if let Some(o) = self.owner {
            b = b.owner(o);
        }
        if let Some(s) = self.owning_scope {
            b = b.owning_scope(s);
        }
        let mut record = b.build().expect("SubtaskBuilder produced invalid Record");
        record.envelope.labels = self.labels;
        record.envelope.state_hash = String::new();
        record.envelope.state_hash =
            ft_core::state_hash(&record).expect("rehash after label append");
        record
    }
}

// ---------------------------------------------------------------------------
// BugBuilder
// ---------------------------------------------------------------------------

/// Fluent builder for a Bug [`Record`].
#[derive(Debug, Clone)]
pub struct BugBuilder {
    title: String,
    description: String,
    status: Status,
    priority: Priority,
    owner: Option<Identity>,
    created_by: Identity,
    service: Option<String>,
    severity: Option<String>,
    acceptance_criteria: Vec<AcceptanceCriterion>,
    labels: Vec<Label>,
    owning_scope: Option<String>,
}

impl Default for BugBuilder {
    fn default() -> Self {
        Self {
            title: "test bug".to_string(),
            description: String::new(),
            status: Status::Open,
            priority: Priority::P2,
            owner: None,
            created_by: make_identity(),
            service: None,
            severity: None,
            acceptance_criteria: Vec::new(),
            labels: Vec::new(),
            owning_scope: None,
        }
    }
}

impl BugBuilder {
    /// Set the title.
    #[must_use]
    pub fn title(mut self, t: impl Into<String>) -> Self {
        self.title = t.into();
        self
    }
    /// Set the description.
    #[must_use]
    pub fn description(mut self, d: impl Into<String>) -> Self {
        self.description = d.into();
        self
    }
    /// Set the status.
    #[must_use]
    pub fn status(mut self, s: Status) -> Self {
        self.status = s;
        self
    }
    /// Set the priority.
    #[must_use]
    pub fn priority(mut self, p: Priority) -> Self {
        self.priority = p;
        self
    }
    /// Set the owner.
    #[must_use]
    pub fn owner(mut self, o: Identity) -> Self {
        self.owner = Some(o);
        self
    }
    /// Override the creator identity.
    #[must_use]
    pub fn created_by(mut self, c: Identity) -> Self {
        self.created_by = c;
        self
    }
    /// Set the affected service identifier.
    #[must_use]
    pub fn service(mut self, s: impl Into<String>) -> Self {
        self.service = Some(s.into());
        self
    }
    /// Set the severity (free-form at M1).
    #[must_use]
    pub fn severity(mut self, s: impl Into<String>) -> Self {
        self.severity = Some(s.into());
        self
    }
    /// Append an acceptance criterion (id auto-assigned as `ac-NN`).
    #[must_use]
    pub fn acceptance_criterion(mut self, text: impl Into<String>) -> Self {
        self.acceptance_criteria
            .push(new_ac(self.acceptance_criteria.len() + 1, text));
        self
    }
    /// Append a free-form label.
    #[must_use]
    pub fn label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.push(Label {
            key: key.into(),
            value: value.into(),
        });
        self
    }
    /// Set the owning scope.
    #[must_use]
    pub fn owning_scope(mut self, scope: impl Into<String>) -> Self {
        self.owning_scope = Some(scope.into());
        self
    }

    /// Finalize into a validated [`Record`].
    #[must_use]
    pub fn build(self) -> Record {
        let body = Bug {
            description: self.description,
            service: self.service,
            severity: self.severity,
            acceptance_criteria: self.acceptance_criteria,
            evidence: Vec::new(),
            claim: None,
        };

        let mut b = RecordBuilder::new(RecordKind::Bug, self.title, self.created_by)
            .status(self.status)
            .priority(self.priority)
            .bug(body);
        if let Some(o) = self.owner {
            b = b.owner(o);
        }
        if let Some(s) = self.owning_scope {
            b = b.owning_scope(s);
        }
        let mut record = b.build().expect("BugBuilder produced invalid Record");
        record.envelope.labels = self.labels;
        record.envelope.state_hash = String::new();
        record.envelope.state_hash =
            ft_core::state_hash(&record).expect("rehash after label append");
        record
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn new_ac(n: usize, text: impl Into<String>) -> AcceptanceCriterion {
    let now = Utc::now();
    AcceptanceCriterion {
        id: format!("ac-{n:02}"),
        text: text.into(),
        status: AcStatus::Unchecked,
        evidence_url: None,
        checked_by: None,
        checked_at: None,
        created_at: now,
        updated_at: now,
        proposed: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_defaults() {
        let r = make_task().build();
        assert_eq!(r.envelope.title, "test task");
        assert_eq!(r.envelope.status, Status::Open);
        assert_eq!(r.envelope.priority, Priority::P2);
        assert!(r.envelope.owner.is_none());
        assert_eq!(r.envelope.kind, RecordKind::Task);
        assert_eq!(r.envelope.state_hash.len(), 64);
    }

    #[test]
    fn epic_defaults() {
        let r = make_epic().build();
        assert_eq!(r.envelope.title, "test epic");
        assert_eq!(r.envelope.kind, RecordKind::Epic);
    }

    #[test]
    fn bug_defaults() {
        let r = make_bug().build();
        assert_eq!(r.envelope.title, "test bug");
        assert_eq!(r.envelope.kind, RecordKind::Bug);
    }

    #[test]
    fn subtask_requires_parent() {
        let parent = make_task().build();
        let sub = make_subtask(parent.envelope.id.clone()).build();
        assert_eq!(sub.envelope.kind, RecordKind::Subtask);
        if let ft_core::RecordBody::Subtask(s) = &sub.body {
            assert_eq!(s.parent_task, parent.envelope.id);
        } else {
            panic!("expected subtask body");
        }
    }

    #[test]
    fn identity_factories_are_valid() {
        let _ = make_identity();
        let alice = make_identity_named("alice");
        assert!(alice.as_str().contains('@'));
        let bob_email = make_identity_named("bob@elsewhere.test");
        assert_eq!(bob_email.as_str(), "bob@elsewhere.test");
    }

    #[test]
    fn task_with_overrides_roundtrips() {
        let r = make_task()
            .title("custom")
            .priority(Priority::P0)
            .status(Status::Ready)
            .acceptance_criterion("first")
            .acceptance_criterion("second")
            .label("area", "search")
            .owning_scope("scope/test")
            .build();
        let json = serde_json::to_string(&r).unwrap();
        let back: Record = serde_json::from_str(&json).unwrap();
        assert_eq!(back, r);
    }
}

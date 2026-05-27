//! Filter type for [`crate::Storage::list`] and [`crate::Storage::iter`].

use chrono::{DateTime, Utc};
use ft_core::{Identity, RecordKind, Status};

/// Filter applied when listing or iterating records.
///
/// Filter fields compose with AND. Each `Vec` field composes with OR among
/// its values:
///
/// ```text
/// kinds = [Task, Bug]
/// statuses = [Open, Ready]
/// ```
///
/// matches a record whose kind is Task **or** Bug, **and** whose status is
/// Open **or** Ready.
///
/// The empty filter (the default) matches every record.
#[derive(Default, Clone, Debug)]
pub struct StorageFilter {
    /// Restrict by record kind.
    pub kinds: Option<Vec<RecordKind>>,
    /// Restrict by record status.
    pub statuses: Option<Vec<Status>>,
    /// Restrict by owner identity.
    pub owners: Option<Vec<Identity>>,
    /// Restrict by owning scope or any affected scope.
    pub scopes: Option<Vec<String>>,
    /// Restrict by `(label.key, label.value)` pairs. AND across pairs.
    pub labels: Vec<(String, String)>,
    /// Only include records whose `updated_at` is at or after the given
    /// timestamp.
    pub modified_since: Option<DateTime<Utc>>,
}

impl StorageFilter {
    /// Restrict to a single record kind. May be called multiple times.
    #[must_use]
    pub fn kind(mut self, k: RecordKind) -> Self {
        self.kinds.get_or_insert_with(Vec::new).push(k);
        self
    }

    /// Restrict to a single status. May be called multiple times.
    #[must_use]
    pub fn status(mut self, s: Status) -> Self {
        self.statuses.get_or_insert_with(Vec::new).push(s);
        self
    }

    /// Restrict to a single owner. May be called multiple times.
    #[must_use]
    pub fn owner(mut self, o: Identity) -> Self {
        self.owners.get_or_insert_with(Vec::new).push(o);
        self
    }

    /// Restrict to a single scope. May be called multiple times.
    #[must_use]
    pub fn scope(mut self, s: impl Into<String>) -> Self {
        self.scopes.get_or_insert_with(Vec::new).push(s.into());
        self
    }

    /// Require a label match. May be called multiple times (AND).
    #[must_use]
    pub fn label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.push((key.into(), value.into()));
        self
    }

    /// Require `updated_at >= ts`.
    #[must_use]
    pub fn modified_since(mut self, ts: DateTime<Utc>) -> Self {
        self.modified_since = Some(ts);
        self
    }

    /// Whether `record` matches this filter.
    #[must_use]
    pub fn matches(&self, record: &ft_core::Record) -> bool {
        let env = &record.envelope;
        if let Some(kinds) = &self.kinds {
            if !kinds.contains(&env.kind) {
                return false;
            }
        }
        if let Some(statuses) = &self.statuses {
            if !statuses.contains(&env.status) {
                return false;
            }
        }
        if let Some(owners) = &self.owners {
            match &env.owner {
                Some(o) if owners.contains(o) => {}
                _ => return false,
            }
        }
        if let Some(scopes) = &self.scopes {
            let owning_match = env
                .owning_scope
                .as_ref()
                .is_some_and(|s| scopes.iter().any(|f| f == s));
            let affected_match = env
                .affected_scopes
                .iter()
                .any(|s| scopes.iter().any(|f| f == s));
            if !owning_match && !affected_match {
                return false;
            }
        }
        for (k, v) in &self.labels {
            if !env.labels.iter().any(|l| &l.key == k && &l.value == v) {
                return false;
            }
        }
        if let Some(ts) = self.modified_since {
            if env.updated_at < ts {
                return false;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ft_core::{Identity, Label, Priority, RecordKind, Status};
    use ft_testkit::{make_bug, make_epic, make_identity_named, make_task};

    #[test]
    fn default_matches_anything() {
        let f = StorageFilter::default();
        assert!(f.matches(&make_task().build()));
        assert!(f.matches(&make_epic().build()));
    }

    #[test]
    fn kind_filter_or_semantics() {
        let f = StorageFilter::default()
            .kind(RecordKind::Task)
            .kind(RecordKind::Bug);
        assert!(f.matches(&make_task().build()));
        assert!(f.matches(&make_bug().build()));
        assert!(!f.matches(&make_epic().build()));
    }

    #[test]
    fn status_filter() {
        let open = make_task().status(Status::Open).build();
        let closed = make_task().status(Status::Closed).build();
        let f = StorageFilter::default().status(Status::Closed);
        assert!(!f.matches(&open));
        assert!(f.matches(&closed));
    }

    #[test]
    fn owner_filter_requires_owner_set() {
        let unowned = make_task().build();
        let alice = make_identity_named("alice");
        let owned = make_task().owner(alice.clone()).build();
        let f = StorageFilter::default().owner(alice);
        assert!(!f.matches(&unowned));
        assert!(f.matches(&owned));
    }

    #[test]
    fn scope_filter_matches_owning_or_affected() {
        let r = make_task().owning_scope("api").build();
        let f = StorageFilter::default().scope("api");
        assert!(f.matches(&r));
        let other = make_task().owning_scope("ui").build();
        assert!(!f.matches(&other));
    }

    #[test]
    fn label_filter_and_semantics() {
        let mut r = make_task().build();
        r.envelope.labels.push(Label {
            key: "area".into(),
            value: "search".into(),
        });
        r.envelope.labels.push(Label {
            key: "team".into(),
            value: "alpha".into(),
        });
        // Re-hash invariant not required for matches(); matches() reads envelope only.
        let f = StorageFilter::default()
            .label("area", "search")
            .label("team", "alpha");
        assert!(f.matches(&r));
        let f2 = StorageFilter::default().label("team", "beta");
        assert!(!f2.matches(&r));
    }

    #[test]
    fn modified_since_filter() {
        let r = make_task().build();
        let before = r.envelope.updated_at - chrono::Duration::seconds(1);
        let after = r.envelope.updated_at + chrono::Duration::seconds(1);
        assert!(StorageFilter::default().modified_since(before).matches(&r));
        assert!(!StorageFilter::default().modified_since(after).matches(&r));
    }

    #[test]
    fn priority_is_ignored_by_filter() {
        // Sanity: filter does not look at priority.
        let r = make_task().priority(Priority::P0).build();
        let _ignore = Identity::new("x@y.test").unwrap();
        assert!(StorageFilter::default().matches(&r));
    }
}

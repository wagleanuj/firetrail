//! Search-layer identity (`DocId`) and kind (`IndexKind`) types.
//!
//! Search indexes more than `ft_core::Record`s: it also indexes synthetic
//! documents for scopes, identities, and per-entry audit history. Those have
//! no `RecordId` (which requires a 64-hex tail, ADR-0015) and no `RecordKind`.
//! These types widen the search surface to cover both.

use ft_core::{RecordId, RecordKind};
use serde::{Serialize, Serializer};

/// Search-layer kind: the record kinds plus the synthetic domains.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IndexKind {
    /// One of the ten `ft_core::RecordKind`s.
    Record(RecordKind),
    /// A scope definition (`.firetrail/scopes.yaml`).
    Scope,
    /// A registered identity (`.firetrail/identities.yaml`).
    Identity,
    /// One audit/history entry of a record.
    Audit,
}

/// Search-layer document id. Records keep their `RecordId`; synthetic docs use
/// a namespaced key.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DocId {
    /// A real record.
    Record(RecordId),
    /// A synthetic document. `key` is domain-specific (scope id, identity id,
    /// or `<record-id>#h<n>`).
    Synthetic {
        /// Which synthetic domain this id belongs to.
        kind: IndexKind,
        /// The domain-specific key.
        key: String,
    },
}

impl IndexKind {
    /// Stable lowercase label (matches `RecordKind`'s serde labels for the
    /// record variants).
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            IndexKind::Record(k) => record_kind_label(k),
            IndexKind::Scope => "scope",
            IndexKind::Identity => "identity",
            IndexKind::Audit => "audit",
        }
    }

    /// Inverse of [`Self::label`]. Returns `None` for unknown labels.
    #[must_use]
    pub fn parse_label(s: &str) -> Option<Self> {
        Some(match s {
            "scope" => IndexKind::Scope,
            "identity" => IndexKind::Identity,
            "audit" => IndexKind::Audit,
            other => IndexKind::Record(record_kind_from_label(other)?),
        })
    }
}

impl Serialize for IndexKind {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.label())
    }
}

impl DocId {
    /// The canonical string form used as the FTS / vec primary key.
    ///
    /// - `Record` → bare `RecordId` string (`TASK-<64hex>`).
    /// - `Synthetic` → `<tag>:<key>` where tag ∈ {scope, identity, audit}.
    #[must_use]
    pub fn as_storage_str(&self) -> String {
        match self {
            DocId::Record(id) => id.as_str().to_string(),
            DocId::Synthetic { kind, key } => format!("{}:{}", kind.label(), key),
        }
    }

    /// Parse the storage form. A string that is a valid `RecordId` → `Record`;
    /// a `<tag>:<key>` string → `Synthetic`. Anything else falls back to an
    /// `Audit` synthetic (so an unknown id never panics — it just won't resolve
    /// metadata). `Audit` is the chosen fallback because the only non-
    /// self-describing keys we emit are audit-history keys.
    ///
    /// `split_once(':')` splits on the first colon only, so a `key` containing
    /// colons round-trips through [`Self::as_storage_str`].
    #[must_use]
    pub fn parse(s: &str) -> Self {
        if let Ok(id) = RecordId::from_string(s) {
            return DocId::Record(id);
        }
        if let Some((tag, key)) = s.split_once(':') {
            if let Some(kind) = IndexKind::parse_label(tag) {
                return DocId::Synthetic {
                    kind,
                    key: key.to_string(),
                };
            }
        }
        DocId::Synthetic {
            kind: IndexKind::Audit,
            key: s.to_string(),
        }
    }

    /// The backing `RecordId`, if this doc is a real record. Synthetic docs
    /// return `None` (used to skip record-only operations like quarantine).
    #[must_use]
    pub fn as_record_id(&self) -> Option<&RecordId> {
        match self {
            DocId::Record(id) => Some(id),
            DocId::Synthetic { .. } => None,
        }
    }
}

impl From<RecordId> for DocId {
    fn from(id: RecordId) -> Self {
        DocId::Record(id)
    }
}

fn record_kind_label(k: RecordKind) -> &'static str {
    match k {
        RecordKind::Epic => "epic",
        RecordKind::Task => "task",
        RecordKind::Subtask => "subtask",
        RecordKind::Bug => "bug",
        RecordKind::Incident => "incident",
        RecordKind::Finding => "finding",
        RecordKind::Runbook => "runbook",
        RecordKind::Decision => "decision",
        RecordKind::Gotcha => "gotcha",
        RecordKind::Memory => "memory",
    }
}

fn record_kind_from_label(s: &str) -> Option<RecordKind> {
    Some(match s {
        "epic" => RecordKind::Epic,
        "task" => RecordKind::Task,
        "subtask" => RecordKind::Subtask,
        "bug" => RecordKind::Bug,
        "incident" => RecordKind::Incident,
        "finding" => RecordKind::Finding,
        "runbook" => RecordKind::Runbook,
        "decision" => RecordKind::Decision,
        "gotcha" => RecordKind::Gotcha,
        "memory" => RecordKind::Memory,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rid() -> RecordId {
        RecordId::from_string(format!("TASK-{}", "a".repeat(64))).unwrap()
    }

    #[test]
    fn index_kind_label_roundtrips() {
        assert_eq!(IndexKind::Scope.label(), "scope");
        assert_eq!(IndexKind::Record(RecordKind::Task).label(), "task");
        assert_eq!(
            IndexKind::parse_label("identity"),
            Some(IndexKind::Identity)
        );
        assert_eq!(
            IndexKind::parse_label("epic"),
            Some(IndexKind::Record(RecordKind::Epic))
        );
        assert_eq!(IndexKind::parse_label("nope"), None);
    }

    #[test]
    fn docid_record_storage_str_is_bare_recordid() {
        let d = DocId::Record(rid());
        assert_eq!(d.as_storage_str(), rid().as_str());
        assert_eq!(DocId::parse(&d.as_storage_str()), d);
        assert_eq!(d.as_record_id(), Some(&rid()));
    }

    #[test]
    fn docid_synthetic_storage_str_is_tagged() {
        let d = DocId::Synthetic {
            kind: IndexKind::Scope,
            key: "apps/checkout".to_string(),
        };
        assert_eq!(d.as_storage_str(), "scope:apps/checkout");
        assert_eq!(DocId::parse("scope:apps/checkout"), d);
        assert_eq!(d.as_record_id(), None);
    }

    #[test]
    fn docid_audit_key_embeds_recordid() {
        let key = format!("{}#h3", rid().as_str());
        let d = DocId::Synthetic {
            kind: IndexKind::Audit,
            key: key.clone(),
        };
        assert_eq!(d.as_storage_str(), format!("audit:{key}"));
        assert_eq!(DocId::parse(&format!("audit:{key}")), d);
    }

    #[test]
    fn docid_synthetic_key_with_colon_roundtrips() {
        // split_once(':') keeps colons in the key intact.
        let d = DocId::parse("scope:org:team");
        assert_eq!(
            d,
            DocId::Synthetic {
                kind: IndexKind::Scope,
                key: "org:team".to_string()
            }
        );
        assert_eq!(d.as_storage_str(), "scope:org:team");
    }

    #[test]
    fn index_kind_serializes_lowercase() {
        let j = serde_json::to_string(&IndexKind::Scope).unwrap();
        assert_eq!(j, "\"scope\"");
        let j = serde_json::to_string(&IndexKind::Record(RecordKind::Bug)).unwrap();
        assert_eq!(j, "\"bug\"");
    }
}

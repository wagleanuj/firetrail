//! History entry kind classification (ADR-0003 §"audit granularity").

use serde::{Deserialize, Serialize};

/// Coarse semantic classification of a history entry.
///
/// `ft-core`'s [`ft_core::HistoryEntry`] does not carry an explicit kind
/// field — at the JSON-in-record level entries are PR-grained compactions
/// (see ADR-0003) whose `ops_summary` lines are free-form. `ft-history`
/// re-introduces an explicit kind to drive compaction (only `Update` is
/// squashable) and to make verification reports human-readable.
///
/// When a [`crate::HistoryDraft`] is appended to a record's history, its
/// `kind` is encoded as the first token of the first `ops_summary` line
/// using the `kebab-case` form returned by [`Self::as_tag`]. Compaction
/// reads that tag back to decide whether an entry is preservable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryEntryKind {
    /// First entry on a record — the create event.
    Create,
    /// Field-level mutation. The only squashable kind.
    Update,
    /// A trust state transition (ADR-0013). Always preserved.
    TrustTransition,
    /// Record closed.
    Close,
    /// Record reopened.
    Reopen,
    /// Record superseded by another record.
    Supersede,
    /// Record marked deprecated.
    Deprecate,
    /// Record archived.
    Archive,
    /// Record redacted (PII / secret scrub).
    Redact,
}

impl HistoryEntryKind {
    /// Stable kebab-case tag used as the first token of the encoded
    /// `ops_summary` line on a [`ft_core::HistoryEntry`].
    #[must_use]
    pub fn as_tag(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Update => "update",
            Self::TrustTransition => "trust-transition",
            Self::Close => "close",
            Self::Reopen => "reopen",
            Self::Supersede => "supersede",
            Self::Deprecate => "deprecate",
            Self::Archive => "archive",
            Self::Redact => "redact",
        }
    }

    /// Inverse of [`Self::as_tag`]. Returns `None` if the tag is unknown.
    #[must_use]
    pub fn from_tag(tag: &str) -> Option<Self> {
        Some(match tag {
            "create" => Self::Create,
            "update" => Self::Update,
            "trust-transition" => Self::TrustTransition,
            "close" => Self::Close,
            "reopen" => Self::Reopen,
            "supersede" => Self::Supersede,
            "deprecate" => Self::Deprecate,
            "archive" => Self::Archive,
            "redact" => Self::Redact,
            _ => return None,
        })
    }

    /// Whether this kind is preserved across compaction by default
    /// (ADR-0003 §"preserve audit-critical entries"). Only `Update` is
    /// squashable.
    #[must_use]
    pub fn is_audit_critical(self) -> bool {
        !matches!(self, Self::Update)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_roundtrips() {
        for k in [
            HistoryEntryKind::Create,
            HistoryEntryKind::Update,
            HistoryEntryKind::TrustTransition,
            HistoryEntryKind::Close,
            HistoryEntryKind::Reopen,
            HistoryEntryKind::Supersede,
            HistoryEntryKind::Deprecate,
            HistoryEntryKind::Archive,
            HistoryEntryKind::Redact,
        ] {
            assert_eq!(HistoryEntryKind::from_tag(k.as_tag()), Some(k));
        }
    }

    #[test]
    fn only_update_is_squashable() {
        assert!(!HistoryEntryKind::Update.is_audit_critical());
        assert!(HistoryEntryKind::Create.is_audit_critical());
        assert!(HistoryEntryKind::TrustTransition.is_audit_critical());
        assert!(HistoryEntryKind::Close.is_audit_critical());
    }
}

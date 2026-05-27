//! Cross-record relationships (FR-019, FR-020).

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::enums::RelationKind;
use crate::id::RecordId;
use crate::identity::Identity;

/// A directed edge between two records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Relation {
    /// Source record (the subject of the relation phrase).
    pub from: RecordId,
    /// Target record (the object of the relation phrase).
    pub to: RecordId,
    /// What kind of relationship this is (FR-020).
    pub kind: RelationKind,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Identity that authored the relation.
    pub created_by: Identity,
}

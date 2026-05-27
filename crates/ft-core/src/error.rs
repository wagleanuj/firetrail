//! Error types for `ft-core`.

use thiserror::Error;

/// Errors returned by `ft-core` operations.
#[derive(Debug, Error)]
pub enum CoreError {
    /// The provided record identifier was malformed.
    #[error("invalid record id: {0}")]
    InvalidId(String),

    /// The provided identity was malformed.
    #[error("invalid identity: {0}")]
    InvalidIdentity(String),

    /// JSON Schema validation rejected the record.
    #[error("schema validation failed: {0}")]
    SchemaValidation(String),

    /// `serde_json` failure during (de)serialization.
    #[error("serde failure: {0}")]
    Serde(#[from] serde_json::Error),

    /// A `RecordBuilder` invariant was violated.
    #[error("invalid record: {0}")]
    InvalidRecord(String),

    /// Attempt to build a memory-kind body that is not yet writable at M1.
    #[error("record kind `{0}` is declared but not writable until M2")]
    NotYetWritable(&'static str),

    /// Provided prefix did not uniquely resolve in the candidate set.
    #[error("ambiguous record id prefix: {0}")]
    AmbiguousPrefix(String),

    /// Provided prefix matched no record in the candidate set.
    #[error("no record id matches prefix: {0}")]
    NoMatch(String),
}

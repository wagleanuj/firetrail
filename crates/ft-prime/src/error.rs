//! Error type for the prime API.

use thiserror::Error;

/// Errors raised by `ft-prime` entry points.
#[derive(Debug, Error)]
pub enum PrimeError {
    /// The target record requested by [`crate::prime_for_task`] is not in the
    /// index (or storage).
    #[error("target record `{0}` not found")]
    TargetNotFound(String),

    /// The query supplied to [`crate::prime_for_query`] was empty after
    /// whitespace trimming.
    #[error("query is empty")]
    EmptyQuery,

    /// A backing storage call failed.
    #[error(transparent)]
    Storage(#[from] ft_storage::StorageError),

    /// An index query failed.
    #[error(transparent)]
    Index(#[from] ft_index::IndexError),
}

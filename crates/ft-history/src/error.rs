//! Error variants for append, verify, and compaction.

use ft_core::CoreError;

/// Errors returned by [`crate::append_history`].
#[derive(Debug, thiserror::Error)]
pub enum HistoryError {
    /// Re-hashing the mutated record failed.
    #[error("core: {0}")]
    Core(#[from] CoreError),

    /// Caller-supplied draft was rejected (e.g. zero `ops_count`).
    #[error("invalid history draft: {0}")]
    InvalidDraft(String),
}

/// Errors returned by [`crate::verify_chain`].
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum VerifyError {
    /// History is empty but the envelope claims a non-genesis state.
    #[error("history is empty")]
    EmptyHistory,

    /// The first history entry has a non-`None` `from_hash` (no genesis
    /// link, see ADR-0017).
    #[error("first history entry must have empty from_hash (got {got})")]
    MissingGenesis {
        /// The non-empty `from_hash` actually observed on the first entry.
        got: String,
    },

    /// Entry `at_index`'s `from_hash` does not match entry `at_index - 1`'s
    /// `to_hash`.
    #[error(
        "broken chain link at history[{at_index}]: from_hash={from_hash} \
         does not match prior to_hash={prior_to_hash}"
    )]
    BrokenLink {
        /// 0-based index of the offending entry within `record.envelope.history`.
        at_index: usize,
        /// `from_hash` claimed by the offending entry.
        from_hash: String,
        /// `to_hash` of the entry preceding the offending one.
        prior_to_hash: String,
    },

    /// The envelope's stored `state_hash` does not equal the canonical
    /// recomputed hash of the record body.
    #[error("state_hash mismatch: stored={stored} computed={computed} (at envelope)")]
    HashMismatch {
        /// Hash stored in the envelope.
        stored: String,
        /// Hash recomputed via [`ft_core::state_hash`].
        computed: String,
        /// Either `"envelope"` or `"history[<i>]"` to describe where the
        /// mismatch was detected.
        at: String,
    },

    /// The envelope's `state_hash` does not match the last history entry's
    /// `to_hash`. The chain itself is internally consistent but the envelope
    /// has been mutated without an `append_history` call.
    #[error("envelope state_hash {envelope} does not match tail history to_hash {tail}")]
    EnvelopeChainDesync {
        /// `state_hash` claimed by the envelope.
        envelope: String,
        /// `to_hash` of the last `history[]` entry.
        tail: String,
    },

    /// Computing the canonical hash failed (very rare; usually a serde
    /// surprise on exotic chrono values).
    #[error("core: {0}")]
    Core(String),
}

impl From<CoreError> for VerifyError {
    fn from(e: CoreError) -> Self {
        Self::Core(e.to_string())
    }
}

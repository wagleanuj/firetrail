//! [`MemoryBody`] — a mutable view over the memory-kind variants of
//! [`ft_core::RecordBody`].
//!
//! `ft-core` deliberately keeps `RecordBody` as a single enum covering both
//! work-tracking kinds (Epic / Task / Subtask / Bug) and memory kinds
//! (Incident / Finding / Runbook / Decision / Gotcha / Memory). The trust
//! state machine only ever runs against the memory kinds: those are the
//! bodies that carry a `trust: TrustState` field. `MemoryBody` is a borrowed
//! discriminant that lets [`crate::apply_transition`] mutate exactly those
//! six variants without needing to inspect or rebuild the enclosing
//! `RecordBody` value.

use ft_core::{
    Decision, Doc, Finding, Gotcha, Incident, Memory, RecordBody, RepoProfileBody, Runbook,
};

use crate::error::TrustError;

/// Mutable view over the six memory-kind bodies that carry trust state.
///
/// Construct with [`MemoryBody::from_record_body`]. Conversion fails for
/// non-memory bodies (`Epic`, `Task`, `Subtask`, `Bug`) with a clear error
/// rather than silently no-op'ing.
#[derive(Debug)]
pub enum MemoryBody<'a> {
    /// Mutable view over an [`Incident`] body.
    Incident(&'a mut Incident),
    /// Mutable view over a [`Finding`] body.
    Finding(&'a mut Finding),
    /// Mutable view over a [`Runbook`] body.
    Runbook(&'a mut Runbook),
    /// Mutable view over a [`Decision`] body.
    Decision(&'a mut Decision),
    /// Mutable view over a [`Gotcha`] body.
    Gotcha(&'a mut Gotcha),
    /// Mutable view over a generic [`Memory`] body.
    Memory(&'a mut Memory),
    /// Mutable view over a file-backed [`Doc`] body.
    Doc(&'a mut Doc),
    /// Mutable view over a [`RepoProfileBody`] (carries trust, no risk class).
    RepoProfile(&'a mut RepoProfileBody),
}

impl<'a> MemoryBody<'a> {
    /// Project a [`RecordBody`] into the memory-kind subset.
    ///
    /// # Errors
    ///
    /// Returns [`TrustError::IllegalTransition`] with both `from`/`to` set to
    /// [`ft_core::TrustState::Draft`] when called on a non-memory body. This
    /// is a programming error — the trust state machine has no meaning for
    /// work-tracking records.
    pub fn from_record_body(body: &'a mut RecordBody) -> Result<Self, TrustError> {
        match body {
            RecordBody::Incident(b) => Ok(Self::Incident(b)),
            RecordBody::Finding(b) => Ok(Self::Finding(b)),
            RecordBody::Runbook(b) => Ok(Self::Runbook(b)),
            RecordBody::Decision(b) => Ok(Self::Decision(b)),
            RecordBody::Gotcha(b) => Ok(Self::Gotcha(b)),
            RecordBody::Memory(b) => Ok(Self::Memory(b)),
            RecordBody::Doc(b) => Ok(Self::Doc(b)),
            RecordBody::RepoProfile(b) => Ok(Self::RepoProfile(b)),
            RecordBody::Epic(_)
            | RecordBody::Task(_)
            | RecordBody::Subtask(_)
            | RecordBody::Bug(_) => Err(TrustError::IllegalTransition {
                from: ft_core::TrustState::Draft,
                to: ft_core::TrustState::Draft,
            }),
        }
    }

    /// Current trust state of the underlying body.
    #[must_use]
    pub fn trust(&self) -> ft_core::TrustState {
        match self {
            Self::Incident(b) => b.trust,
            Self::Finding(b) => b.trust,
            Self::Runbook(b) => b.trust,
            Self::Decision(b) => b.trust,
            Self::Gotcha(b) => b.trust,
            Self::Memory(b) => b.trust,
            Self::Doc(b) => b.trust,
            Self::RepoProfile(b) => b.trust,
        }
    }

    /// Current risk class of the underlying body.
    #[must_use]
    pub fn risk_class(&self) -> Option<ft_core::RiskClass> {
        match self {
            Self::Incident(b) => b.risk_class,
            Self::Finding(b) => b.risk_class,
            Self::Runbook(b) => b.risk_class,
            Self::Decision(b) => b.risk_class,
            Self::Gotcha(b) => b.risk_class,
            Self::Memory(b) => b.risk_class,
            // Docs and repo profiles carry trust but no risk classification.
            Self::Doc(_) | Self::RepoProfile(_) => None,
        }
    }
}

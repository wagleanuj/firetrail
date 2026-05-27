//! Quarantine label semantics (ADR-0014).
//!
//! Imported records carry the label `quarantine=true`. Callers in `ft-search`
//! and `ft-prime` exclude these by default. The label representation (rather
//! than a dedicated envelope field) keeps the canonical JSON schema and the
//! state-hash form unchanged.

use ft_core::Record;

/// Label key marking an imported record as quarantined.
pub const QUARANTINE_LABEL_KEY: &str = "quarantine";

/// Label value paired with [`QUARANTINE_LABEL_KEY`].
pub const QUARANTINE_LABEL_VALUE: &str = "true";

/// Label key recording the originating import system.
///
/// The value is one of [`crate::SourceSystem::tag`]'s stable strings.
pub const IMPORT_SOURCE_LABEL_KEY: &str = "import:source";

/// Whether `record` is currently quarantined.
///
/// Returns `true` iff the envelope carries the
/// `quarantine=true` label.
#[must_use]
pub fn is_quarantined(record: &Record) -> bool {
    record
        .envelope
        .labels
        .iter()
        .any(|l| l.key == QUARANTINE_LABEL_KEY && l.value == QUARANTINE_LABEL_VALUE)
}

//! Convert parsed markdown into quarantined `ft-core` records.
//!
//! Every record produced here carries the [`crate::QUARANTINE_LABEL_KEY`]
//! label plus an `import:source=<system>` label and `origin: Imported`.

use chrono::{DateTime, Utc};
use ft_core::{
    Decision, DecisionStatus, Identity, Incident, Label, Origin, Record, RecordBuilder, RecordKind,
    Runbook, RunbookStep as CoreRunbookStep, Severity, TrustState, state_hash,
};

use crate::error::ImportError;
use crate::parse::{ParsedAdr, ParsedIncident, ParsedRunbook};
use crate::quarantine::{IMPORT_SOURCE_LABEL_KEY, QUARANTINE_LABEL_KEY, QUARANTINE_LABEL_VALUE};
use crate::source::ImportSource;

/// Builder-side knobs shared by every parser-to-record conversion.
#[derive(Debug, Clone)]
pub struct BuilderOpts {
    /// Identity to record as `created_by`.
    pub created_by: Identity,
    /// Wall-clock time to stamp on the record.
    pub now: DateTime<Utc>,
    /// Provenance metadata for the source artefact.
    pub source: ImportSource,
}

impl BuilderOpts {
    /// Convenience constructor that stamps `now = Utc::now()`.
    #[must_use]
    pub fn new(created_by: Identity, source: ImportSource) -> Self {
        Self {
            created_by,
            now: Utc::now(),
            source,
        }
    }
}

fn quarantine_labels(source: &ImportSource) -> Vec<Label> {
    let mut labels = vec![
        Label {
            key: QUARANTINE_LABEL_KEY.to_string(),
            value: QUARANTINE_LABEL_VALUE.to_string(),
        },
        Label {
            key: IMPORT_SOURCE_LABEL_KEY.to_string(),
            value: source.system.tag().to_string(),
        },
    ];
    if let Some(url) = &source.url {
        labels.push(Label {
            key: "import:url".to_string(),
            value: url.clone(),
        });
    }
    if let Some(path) = &source.file_path {
        labels.push(Label {
            key: "import:path".to_string(),
            value: path.display().to_string(),
        });
    }
    labels
}

/// Re-hash a record after labels were mutated externally.
fn rehash(record: &mut Record) -> Result<(), ImportError> {
    record.envelope.state_hash.clear();
    record.envelope.state_hash = state_hash(record)?;
    Ok(())
}

/// Build an Incident record from a parsed markdown incident.
///
/// # Errors
///
/// Bubbles up [`ImportError::Core`] if the builder rejects the synthesized
/// record (e.g. empty title after trimming).
pub fn parsed_incident_to_record(
    parsed: &ParsedIncident,
    opts: &BuilderOpts,
) -> Result<Record, ImportError> {
    let body = Incident {
        summary: parsed
            .summary
            .clone()
            .unwrap_or_else(|| parsed.title.clone()),
        severity: Severity::default(),
        started_at: opts.now,
        resolved_at: None,
        services_affected: Vec::new(),
        root_cause: parsed.root_cause.clone(),
        findings: Vec::new(),
        runbooks_invoked: Vec::new(),
        risk_class: None,
        trust: TrustState::Draft,
    };

    let mut record = RecordBuilder::new(
        RecordKind::Incident,
        parsed.title.trim(),
        opts.created_by.clone(),
    )
    .origin(Origin::Imported)
    .created_at(opts.now)
    .incident(body)
    .build()?;

    record.envelope.labels = quarantine_labels(&opts.source);
    rehash(&mut record)?;
    Ok(record)
}

/// Build a Decision record from a parsed ADR.
///
/// # Errors
///
/// Bubbles up [`ImportError::Core`] on builder rejection.
pub fn parsed_adr_to_record(parsed: &ParsedAdr, opts: &BuilderOpts) -> Result<Record, ImportError> {
    let status = parse_decision_status(parsed.status.as_deref());

    let body = Decision {
        title: parsed.title.clone(),
        context: parsed.context.clone().unwrap_or_default(),
        decision: parsed.decision.clone().unwrap_or_default(),
        consequences: parsed.consequences.clone().unwrap_or_default(),
        alternatives_considered: parsed.alternatives.clone(),
        status,
        superseded_by: None,
        risk_class: None,
        trust: TrustState::Draft,
    };

    let mut record = RecordBuilder::new(
        RecordKind::Decision,
        parsed.title.trim(),
        opts.created_by.clone(),
    )
    .origin(Origin::Imported)
    .created_at(opts.now)
    .decision(body)
    .build()?;

    record.envelope.labels = quarantine_labels(&opts.source);
    if let Some(n) = parsed.number {
        record.envelope.labels.push(Label {
            key: "adr:number".to_string(),
            value: n.to_string(),
        });
    }
    rehash(&mut record)?;
    Ok(record)
}

/// Best-effort mapping of an ADR `## Status` blurb to a
/// [`DecisionStatus`]. Returns the default if no keyword matches.
fn parse_decision_status(blurb: Option<&str>) -> DecisionStatus {
    let Some(text) = blurb else {
        return DecisionStatus::default();
    };
    let lower = text.to_lowercase();
    if lower.contains("accepted") {
        DecisionStatus::Accepted
    } else if lower.contains("superseded") {
        DecisionStatus::Superseded
    } else if lower.contains("deprecated") {
        DecisionStatus::Deprecated
    } else {
        DecisionStatus::Proposed
    }
}

/// Build a Runbook record from a parsed markdown runbook.
///
/// # Errors
///
/// Bubbles up [`ImportError::Core`] on builder rejection.
pub fn parsed_runbook_to_record(
    parsed: &ParsedRunbook,
    opts: &BuilderOpts,
) -> Result<Record, ImportError> {
    let steps: Vec<CoreRunbookStep> = parsed
        .steps
        .iter()
        .map(|s| CoreRunbookStep {
            description: s.description.clone(),
            command: None,
            expected_outcome: String::new(),
        })
        .collect();

    let body = Runbook {
        title: parsed.title.clone(),
        summary: parsed.summary.clone().unwrap_or_default(),
        steps,
        applies_to: parsed.applies_to.clone(),
        risk_class: None,
        trust: TrustState::Draft,
    };

    let mut record = RecordBuilder::new(
        RecordKind::Runbook,
        parsed.title.trim(),
        opts.created_by.clone(),
    )
    .origin(Origin::Imported)
    .created_at(opts.now)
    .runbook(body)
    .build()?;

    record.envelope.labels = quarantine_labels(&opts.source);
    rehash(&mut record)?;
    Ok(record)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::{parse_adr_md, parse_incident_md, parse_runbook_md};
    use crate::quarantine::is_quarantined;

    fn opts() -> BuilderOpts {
        BuilderOpts::new(
            Identity::new("imp@firetrail.test").unwrap(),
            ImportSource::local_markdown("incidents/redis.md"),
        )
    }

    const INCIDENT: &str = "# Title

## Symptoms

s

## Root Cause

c

## Resolution

r

## Action Items

- a1
- a2

## Lessons Learned

l
";

    #[test]
    fn parsed_incident_to_record_round_trips() {
        let parsed = parse_incident_md(INCIDENT, &ImportSource::local_markdown("x.md")).unwrap();
        let rec = parsed_incident_to_record(&parsed, &opts()).unwrap();
        assert_eq!(rec.envelope.kind, RecordKind::Incident);
        assert_eq!(rec.envelope.origin, Origin::Imported);
        assert!(is_quarantined(&rec));
        // serde round-trip.
        let json = serde_json::to_string(&rec).unwrap();
        let back: Record = serde_json::from_str(&json).unwrap();
        assert_eq!(back, rec);
        // hash matches.
        assert_eq!(state_hash(&rec).unwrap(), rec.envelope.state_hash);
    }

    #[test]
    fn parsed_adr_to_record_sets_status_accepted() {
        let input = "# ADR-1: Title\n\n## Status\n\nAccepted\n\n## Decision\n\nDo it.\n";
        let parsed = parse_adr_md(input, &ImportSource::local_markdown("x.md")).unwrap();
        let rec = parsed_adr_to_record(&parsed, &opts()).unwrap();
        assert!(is_quarantined(&rec));
        if let ft_core::RecordBody::Decision(d) = &rec.body {
            assert_eq!(d.status, DecisionStatus::Accepted);
            assert_eq!(d.title, "Title");
        } else {
            panic!("expected Decision body");
        }
        let json = serde_json::to_string(&rec).unwrap();
        let back: Record = serde_json::from_str(&json).unwrap();
        assert_eq!(back, rec);
    }

    #[test]
    fn parsed_runbook_to_record_round_trips() {
        let input = "# Title\n\n## Summary\n\ns\n\n## Steps\n\n- a\n- b\n";
        let parsed = parse_runbook_md(input, &ImportSource::local_markdown("x.md")).unwrap();
        let rec = parsed_runbook_to_record(&parsed, &opts()).unwrap();
        assert!(is_quarantined(&rec));
        if let ft_core::RecordBody::Runbook(rb) = &rec.body {
            assert_eq!(rb.steps.len(), 2);
        } else {
            panic!("expected Runbook body");
        }
    }
}

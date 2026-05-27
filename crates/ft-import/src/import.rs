//! Directory-walking import workflow.
//!
//! Walks a directory for `*.md` files, parses each according to
//! [`ImportKind`], converts to quarantined records, and either writes them
//! (apply mode) or reports without writing (dry-run mode).

use std::fs;
use std::path::{Path, PathBuf};

use ft_core::{Identity, Record, RecordId};
use ft_storage::Storage;

use crate::convert::{
    BuilderOpts, parsed_adr_to_record, parsed_incident_to_record, parsed_runbook_to_record,
};
use crate::error::ImportError;
use crate::parse::{parse_adr_md, parse_incident_md, parse_runbook_md};
use crate::source::ImportSource;

/// What kind of content the directory holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportKind {
    /// Markdown incident reports.
    Incidents,
    /// Markdown ADRs.
    Adrs,
    /// Markdown runbooks.
    Runbooks,
}

/// Options modifying [`import_dir`] behaviour.
#[derive(Debug, Clone)]
pub struct ImportOptions {
    /// Identity stamped on synthesized records.
    pub created_by: Identity,
    /// If `true`, parse and report; do not write to storage.
    pub dry_run: bool,
    /// If `true`, parse and write to storage. Ignored when `dry_run` is set.
    pub apply: bool,
}

impl ImportOptions {
    /// Construct a dry-run-by-default options bag.
    #[must_use]
    pub fn new(created_by: Identity) -> Self {
        Self {
            created_by,
            dry_run: true,
            apply: false,
        }
    }
}

/// Summary of an [`import_dir`] invocation.
#[derive(Debug, Default)]
pub struct ImportReport {
    /// Total `*.md` files visited.
    pub files_seen: usize,
    /// Files that parsed successfully.
    pub parsed: usize,
    /// Records actually written to storage. Always `0` in dry-run mode.
    pub written: usize,
    /// `(path, reason)` for each file that failed.
    pub failures: Vec<(PathBuf, String)>,
    /// IDs of records produced (written or that would have been written).
    pub records: Vec<RecordId>,
}

/// Import every `*.md` file under `dir`.
///
/// If `opts.dry_run` is set (the default), nothing is written and the report
/// describes what would have been imported. If `opts.apply` is set without
/// `dry_run`, parsed records are written to `storage` with the quarantine
/// label set.
///
/// # Errors
///
/// Per-file errors are collected into [`ImportReport::failures`] rather than
/// aborting the whole walk. The function returns `Err` only on an
/// unrecoverable I/O failure walking the root directory.
pub fn import_dir(
    storage: &dyn Storage,
    dir: &Path,
    kind: ImportKind,
    opts: &ImportOptions,
) -> Result<ImportReport, ImportError> {
    if !dir.is_dir() {
        return Err(ImportError::Io {
            path: dir.to_path_buf(),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "not a directory"),
        });
    }

    let mut report = ImportReport::default();
    for entry in walkdir::WalkDir::new(dir).follow_links(false) {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                report
                    .failures
                    .push((dir.to_path_buf(), format!("walk error: {e}")));
                continue;
            }
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "md") {
            continue;
        }
        report.files_seen += 1;

        match import_one(storage, path, kind, opts) {
            Ok((id, wrote)) => {
                report.parsed += 1;
                report.records.push(id);
                if wrote {
                    report.written += 1;
                }
            }
            Err(e) => {
                report.failures.push((path.to_path_buf(), e.to_string()));
            }
        }
    }
    Ok(report)
}

fn import_one(
    storage: &dyn Storage,
    path: &Path,
    kind: ImportKind,
    opts: &ImportOptions,
) -> Result<(RecordId, bool), ImportError> {
    let content = fs::read_to_string(path).map_err(|source| ImportError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let source = ImportSource::local_markdown(path);
    let bopts = BuilderOpts::new(opts.created_by.clone(), source.clone());

    let record: Record = match kind {
        ImportKind::Incidents => {
            let parsed = parse_incident_md(&content, &source)?;
            parsed_incident_to_record(&parsed, &bopts)?
        }
        ImportKind::Adrs => {
            let parsed = parse_adr_md(&content, &source)?;
            parsed_adr_to_record(&parsed, &bopts)?
        }
        ImportKind::Runbooks => {
            let parsed = parse_runbook_md(&content, &source)?;
            parsed_runbook_to_record(&parsed, &bopts)?
        }
    };

    let id = record.envelope.id.clone();
    let wrote = if !opts.dry_run && opts.apply {
        storage.write(&record)?;
        true
    } else {
        false
    };
    Ok((id, wrote))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quarantine::is_quarantined;
    use ft_storage::EmbeddedStorage;
    use ft_testkit::TestRepo;
    use std::fs;

    fn write_file(dir: &Path, name: &str, body: &str) {
        let p = dir.join(name);
        fs::write(p, body).unwrap();
    }

    const INCIDENT_A: &str = "# Incident A\n\n## Symptoms\n\ns\n\n## Root Cause\n\nc\n\n## Resolution\n\nr\n\n## Action Items\n\n- a\n\n## Lessons Learned\n\nl\n";
    const INCIDENT_B: &str = "# Incident B\n\n## Symptoms\n\ns2\n\n## Resolution\n\nr2\n";
    const INCIDENT_C: &str = "# Incident C\n\n## Symptoms\n\ns3\n";

    #[test]
    fn import_dir_dry_run_does_not_write() {
        let tr = TestRepo::new().unwrap();
        let storage = EmbeddedStorage::open(tr.root()).unwrap();
        let dir = tr.root().join("imports");
        fs::create_dir_all(&dir).unwrap();
        write_file(&dir, "a.md", INCIDENT_A);
        write_file(&dir, "b.md", INCIDENT_B);
        write_file(&dir, "c.md", INCIDENT_C);

        let opts = ImportOptions::new(Identity::new("imp@firetrail.test").unwrap());
        let report = import_dir(&storage, &dir, ImportKind::Incidents, &opts).unwrap();
        assert_eq!(report.files_seen, 3);
        assert_eq!(report.parsed, 3);
        assert_eq!(report.written, 0);
        assert_eq!(report.records.len(), 3);
        // No record was actually written.
        for id in &report.records {
            assert!(storage.read(id).is_err());
        }
    }

    #[test]
    fn import_dir_apply_writes_quarantined_records() {
        let tr = TestRepo::new().unwrap();
        let storage = EmbeddedStorage::open(tr.root()).unwrap();
        let dir = tr.root().join("imports");
        fs::create_dir_all(&dir).unwrap();
        write_file(&dir, "a.md", INCIDENT_A);
        write_file(&dir, "b.md", INCIDENT_B);

        let mut opts = ImportOptions::new(Identity::new("imp@firetrail.test").unwrap());
        opts.dry_run = false;
        opts.apply = true;
        let report = import_dir(&storage, &dir, ImportKind::Incidents, &opts).unwrap();
        assert_eq!(report.written, 2);
        for id in &report.records {
            let r = storage.read(id).unwrap();
            assert!(is_quarantined(&r));
        }
    }

    #[test]
    fn import_dir_missing_directory_errors() {
        let tr = TestRepo::new().unwrap();
        let storage = EmbeddedStorage::open(tr.root()).unwrap();
        let opts = ImportOptions::new(Identity::new("imp@firetrail.test").unwrap());
        let err = import_dir(
            &storage,
            &tr.root().join("no-such-dir"),
            ImportKind::Incidents,
            &opts,
        )
        .unwrap_err();
        assert!(matches!(err, ImportError::Io { .. }));
    }
}

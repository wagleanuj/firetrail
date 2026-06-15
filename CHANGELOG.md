# Changelog

All notable changes to Firetrail are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## 0.2.5 - 2026-06-15

### Added

- **`firetrail upgrade`:** self-update the installed binary to the latest
  GitHub release, with `firetrail upgrade --check` to report availability
  without installing. Backed by `axoupdater`; works for binaries installed via
  the Firetrail installer (which records an install receipt) and prints
  actionable guidance otherwise. Named `upgrade` to avoid colliding with the
  existing record `update` command (firetrail-7oic).

## 0.2.4 - 2026-06-14

### Fixed

- **UI:** Clicking an `audit`, `scope`, or `identity` search hit in memory
  search no longer 404s with "memory not found". Memory search previously
  linked every hit to `/memory/$id` by its raw document id, but synthetic
  documents (per-history-entry audit echoes `audit:<RecordId>#h<n>`, scope
  and identity docs) are not memory records. Hits are now routed through the
  shared `resultTarget()` helper used by the command palette; audit echoes
  resolve to their underlying record on the correct surface (tickets or
  memory) (firetrail-g5n6).

## 0.2.3 - 2026-06-09

### Fixed

- **External storage:** ft-ops, the UI, and `index rebuild` now read the
  data-repo clone in external mode, so tickets and memory records stored in a
  separate data repository display correctly (firetrail-zkme, #2).

## 0.2.2 - 2026-06-02

### Changed

- Maintenance release.

## 0.2.1 - 2026-06-02

### Fixed

- **Import:** Repaired the promotion chain, fixed the `--force` no-op, and
  corrected `parse_confidence` reporting (#1).

## 0.2.0 - 2026-06-01

### Added

- Pure-Rust ONNX embedding backend (tract), enabling the ONNX-enabled build to
  link on every supported target with no platform-specific prebuilt runtime
  (firetrail-huvf).

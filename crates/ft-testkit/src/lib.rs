//! # ft-testkit
//!
//! Shared test fixtures, record factories, assertion helpers, and the
//! scenario runner skeleton used by every Firetrail crate's tests.
//!
//! See `docs/components/ft-testkit.md` for the authoritative spec.
//!
//! ## Surface
//!
//! - [`TestRepo`] — isolated tempdir-backed Firetrail workspace
//! - [`make_task`], [`make_epic`], [`make_subtask`], [`make_bug`] — record factories
//! - [`make_identity`], [`make_identity_named`] — identity factories
//! - [`assert_record_exists`], [`assert_field`], [`assert_relation`],
//!   [`assert_hash_consistent`], [`dump_workspace`] — assertion helpers
//! - [`ScenarioRunner`] — YAML scenario file runner skeleton
//! - [`MockEmbedder`] — deterministic embedder stub (M3 fills in)
//!
//! ## Relevant ADRs
//!
//! - ADR-0016 — Build approach (five-layer test harness)

pub mod assertions;
pub mod error;
pub mod factories;
pub mod mock_embedder;
pub mod paths;
pub mod repo;
pub mod scenario;

pub use assertions::{
    assert_field, assert_hash_consistent, assert_record_exists, assert_relation, dump_workspace,
};
pub use error::{ScenarioError, ScenarioFailure, ScenarioReport, TestKitError};
pub use factories::{
    BugBuilder, EpicBuilder, SubtaskBuilder, TaskBuilder, make_bug, make_epic, make_identity,
    make_identity_named, make_subtask, make_task,
};
pub use mock_embedder::MockEmbedder;
pub use repo::{CmdOutput, StorageMode, TestRepo, TestRepoConfig};
pub use scenario::{RunnerOptions, ScenarioRunner};

//! M1 end-to-end scenario suite.
//!
//! Each YAML file under `tests/scenarios/` is driven by ft-testkit's
//! [`ScenarioRunner`] against a fresh tempdir-backed workspace. The runner
//! locates the `firetrail` binary via the compile-time
//! `CARGO_BIN_EXE_firetrail` env var that Cargo sets for this integration
//! target (ft-cli is the test target's own crate). The path is injected into
//! [`RunnerOptions`] so the runner itself does not need a dep on ft-cli.
//!
//! ## Why this lives in `ft-cli/tests/` (not the workspace root)
//!
//! ft-testkit cannot depend on ft-cli (would create a workspace cycle: every
//! crate's tests pull in ft-testkit, and ft-cli depends on most of them).
//! Cargo only sets `CARGO_BIN_EXE_firetrail` in test targets that have ft-cli
//! in their dep graph. The cleanest placement is ft-cli's own
//! `tests/scenarios.rs`, where the binary is guaranteed to be built before
//! the test runs.
//!
//! ## Failure presentation
//!
//! Each scenario runs as its own `#[test]`. A failure prints the offending
//! step's name, the assertion message, and (when captured) a dump of the
//! workspace state.

use std::path::PathBuf;

use ft_testkit::{RunnerOptions, ScenarioRunner};

const SCENARIO_DIR: &str = "tests/scenarios";

fn runner_options() -> RunnerOptions {
    let bin = env!("CARGO_BIN_EXE_firetrail");
    // M4 introduces a second binary target — the standalone merge driver git
    // invokes via `%O %A %B`. Scenarios that exercise the driver outside the
    // `firetrail merge-driver-install` shim (m4-merge-driver) need its
    // absolute path. Cargo sets `CARGO_BIN_EXE_firetrail-merge-driver`
    // alongside the primary `CARGO_BIN_EXE_firetrail` env var for the test
    // target; we plumb it through the runner so YAML steps can reference it
    // as `$FIRETRAIL_MERGE_DRIVER_BIN` from `sh -c`.
    let merge_driver = env!("CARGO_BIN_EXE_firetrail-merge-driver");
    RunnerOptions::default()
        .with_firetrail_bin(bin)
        // Pin identity so the scenario is host-agnostic.
        .with_env("FIRETRAIL_AUTHOR", "alice@example.com")
        // Expose the resolved binary path to shell-out steps (M3 daemon
        // scenario needs to spawn `firetrail daemon start --foreground` in
        // the background via `sh -c`, and the scenario runner only rewrites
        // the literal `firetrail` head argv when it dispatches directly).
        .with_env("FIRETRAIL_BIN", bin)
        .with_env("FIRETRAIL_MERGE_DRIVER_BIN", merge_driver)
}

fn scenario_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(SCENARIO_DIR)
        .join(name)
}

/// Run a single scenario and panic with a structured diagnostic if any step
/// failed. The diagnostic is designed to make CI failure logs actionable.
fn run_scenario(name: &str) {
    run_scenario_with_budget(name, std::time::Duration::from_secs(15));
}

/// Variant that takes an explicit per-scenario runtime budget. M2 scenarios
/// drive more git operations than M1 and are sized to the M2 plan's 10s
/// cap; the M1 surface stays pinned at 5s.
fn run_scenario_with_budget(name: &str, budget: std::time::Duration) {
    let path = scenario_path(name);
    assert!(path.exists(), "scenario file missing: {}", path.display());
    let opts = runner_options();
    let report = ScenarioRunner::run_with_options(&path, &opts)
        .unwrap_or_else(|e| panic!("scenario `{name}` failed to load/run: {e}"));

    if !report.failures.is_empty() {
        use std::fmt::Write as _;
        let mut msg = format!(
            "scenario `{}` had {} failing step(s) (elapsed {:?}):\n",
            report.name,
            report.failures.len(),
            report.elapsed,
        );
        for f in &report.failures {
            let _ = write!(
                msg,
                "\n--- step #{}: {} ---\n{}\n",
                f.step_index, f.step_description, f.message,
            );
            if let Some(dump) = &f.workspace_dump {
                msg.push_str("--- workspace dump ---\n");
                msg.push_str(dump);
                msg.push('\n');
            }
        }
        panic!("{msg}");
    }

    assert_eq!(
        report.steps_passed, report.steps_run,
        "scenario `{}` reports fewer passes than steps",
        report.name
    );
    eprintln!(
        "scenario `{}` passed {}/{} steps in {:?}",
        report.name, report.steps_passed, report.steps_run, report.elapsed,
    );

    assert!(
        report.elapsed < budget,
        "scenario `{}` exceeded {:?} budget: {:?}",
        report.name,
        budget,
        report.elapsed,
    );
}

#[test]
fn m1_happy_path() {
    run_scenario("m1-happy-path.yml");
}

#[test]
fn m1_ac_enforcement() {
    run_scenario("m1-ac-enforcement.yml");
}

#[test]
fn m1_conflict_resolution() {
    run_scenario("m1-conflict-resolution.yml");
}

#[test]
fn m1_index_rebuild() {
    run_scenario("m1-index-rebuild.yml");
}

#[test]
fn m1_clean_clone() {
    run_scenario("m1-clean-clone.yml");
}

// ---------------------------------------------------------------------------
// M2 scenarios — incident memory, trust transitions, force-push detection,
// branch salvage. Each is allowed up to 10s (M2 plan) because git commits
// and branch checkouts add unavoidable per-step overhead vs the M1 happy
// paths. The total scenario suite target is well under 60s.
// ---------------------------------------------------------------------------

const M2_BUDGET: std::time::Duration = std::time::Duration::from_secs(10);

#[test]
fn m2_incident_finding_flow() {
    run_scenario_with_budget("m2-incident-finding-flow.yml", M2_BUDGET);
}

#[test]
fn m2_trust_transitions() {
    run_scenario_with_budget("m2-trust-transitions.yml", M2_BUDGET);
}

#[test]
fn m2_force_push_detection() {
    run_scenario_with_budget("m2-force-push-detection.yml", M2_BUDGET);
}

#[test]
fn m2_salvage() {
    run_scenario_with_budget("m2-salvage.yml", M2_BUDGET);
}

// ---------------------------------------------------------------------------
// M3 scenarios — lexical search, trust weighting / filtering, prime budget &
// query, daemon lifecycle, and index rebuild on the M3 surface. ADR/roadmap
// references live in each YAML's `description:` block. Per-scenario budget
// is 15s (roadmap M3 plan); the M3 suite stays under 90s total.
// ---------------------------------------------------------------------------

const M3_BUDGET: std::time::Duration = std::time::Duration::from_secs(15);

#[test]
fn m3_search_lexical() {
    run_scenario_with_budget("m3-search-lexical.yml", M3_BUDGET);
}

#[test]
fn m3_trust_weighting() {
    run_scenario_with_budget("m3-trust-weighting.yml", M3_BUDGET);
}

#[test]
fn m3_prime_budget() {
    run_scenario_with_budget("m3-prime-budget.yml", M3_BUDGET);
}

#[test]
fn m3_prime_query() {
    run_scenario_with_budget("m3-prime-query.yml", M3_BUDGET);
}

#[test]
fn m3_daemon_lifecycle() {
    run_scenario_with_budget("m3-daemon-lifecycle.yml", M3_BUDGET);
}

#[test]
fn m3_index_rebuild() {
    run_scenario_with_budget("m3-index-rebuild.yml", M3_BUDGET);
}

// ---------------------------------------------------------------------------
// M4 scenarios — ft-pr check pr rule coverage end-to-end:
//   - incomplete acceptance (ADR-0010)
//   - evidence-required happy path (ADR-0013 backstop)
//   - mixed commit (ADR-0009)
//   - secret-scan default pattern set
//   - merge driver three-way merge on concurrent AC edits
//   - force-push / on-disk history tamper detection via `verify --all`
//
// Per-scenario budget is 15s (each runs several git commits, a merge, and
// optionally a python tamper step); the M4 suite stays well under 90s total,
// matching the M4 plan's CI-suite envelope.
// ---------------------------------------------------------------------------

const M4_BUDGET: std::time::Duration = std::time::Duration::from_secs(15);

#[test]
fn m4_incomplete_ac() {
    run_scenario_with_budget("m4-incomplete-ac.yml", M4_BUDGET);
}

#[test]
fn m4_evidence_required() {
    run_scenario_with_budget("m4-evidence-required.yml", M4_BUDGET);
}

#[test]
fn m4_mixed_commit() {
    run_scenario_with_budget("m4-mixed-commit.yml", M4_BUDGET);
}

#[test]
fn m4_secret_scan() {
    run_scenario_with_budget("m4-secret-scan.yml", M4_BUDGET);
}

#[test]
fn m4_merge_driver() {
    run_scenario_with_budget("m4-merge-driver.yml", M4_BUDGET);
}

#[test]
fn m4_force_push_detection() {
    run_scenario_with_budget("m4-force-push-detection.yml", M4_BUDGET);
}

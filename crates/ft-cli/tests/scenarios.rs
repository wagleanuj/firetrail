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
    RunnerOptions::default()
        .with_firetrail_bin(bin)
        // Pin identity so the scenario is host-agnostic.
        .with_env("FIRETRAIL_AUTHOR", "alice@example.com")
}

fn scenario_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(SCENARIO_DIR)
        .join(name)
}

/// Run a single scenario and panic with a structured diagnostic if any step
/// failed. The diagnostic is designed to make CI failure logs actionable.
fn run_scenario(name: &str) {
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

    // Per spec, each scenario must stay under 5 seconds locally.
    assert!(
        report.elapsed.as_secs() < 5,
        "scenario `{}` exceeded 5s budget: {:?}",
        report.name,
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

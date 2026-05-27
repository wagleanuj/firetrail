//! Error and reporting types for ft-testkit.

use std::time::Duration;

use thiserror::Error;

/// Errors returned by [`crate::TestRepo`] operations and helpers.
#[derive(Debug, Error)]
pub enum TestKitError {
    /// I/O failure.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// A git command exited non-zero or produced unexpected output.
    #[error("git error: {0}")]
    Git(String),
    /// A spawned command exited non-zero or could not be launched.
    #[error("command failed: {0}")]
    Cmd(String),
    /// A configuration value was invalid.
    #[error("invalid config: {0}")]
    Config(String),
    /// JSON (de)serialization failure when reading/writing records.
    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),
    /// A `ft-core` operation failed.
    #[error("core error: {0}")]
    Core(String),
}

impl From<ft_core::CoreError> for TestKitError {
    fn from(e: ft_core::CoreError) -> Self {
        Self::Core(e.to_string())
    }
}

/// Errors returned by [`crate::ScenarioRunner`].
#[derive(Debug, Error)]
pub enum ScenarioError {
    /// Failed to parse the YAML scenario document.
    #[error("parse error: {0}")]
    Parse(String),
    /// One or more steps failed during execution.
    #[error("step failed: {0}")]
    StepFailed(String),
    /// Setup phase failed before any steps could run.
    #[error("setup failed: {0}")]
    Setup(String),
    /// Underlying [`TestKitError`].
    #[error(transparent)]
    TestKit(#[from] TestKitError),
}

impl From<serde_yaml::Error> for ScenarioError {
    fn from(e: serde_yaml::Error) -> Self {
        Self::Parse(e.to_string())
    }
}

impl From<std::io::Error> for ScenarioError {
    fn from(e: std::io::Error) -> Self {
        Self::TestKit(TestKitError::Io(e))
    }
}

/// Summary report of a scenario run.
#[derive(Debug, Clone)]
pub struct ScenarioReport {
    /// Scenario `name` field from the YAML doc.
    pub name: String,
    /// Number of steps executed (incl. failed).
    pub steps_run: usize,
    /// Number of steps whose expectations all passed.
    pub steps_passed: usize,
    /// Captured failures.
    pub failures: Vec<ScenarioFailure>,
    /// Wall-clock duration of the run.
    pub elapsed: Duration,
}

/// One step failure captured during a scenario run.
#[derive(Debug, Clone)]
pub struct ScenarioFailure {
    /// 0-based index of the failing step in the scenario.
    pub step_index: usize,
    /// `name` of the failing step.
    pub step_description: String,
    /// Human-readable failure message.
    pub message: String,
    /// Optional workspace dump captured at failure time.
    pub workspace_dump: Option<String>,
}

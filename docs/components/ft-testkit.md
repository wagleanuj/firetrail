# ft-testkit — shared test fixtures, factories, scenario runner

**Epic:** `firetrail-5xq`
**Wave:** 1
**Depends on:** workspace skeleton, ft-core (for record types)
**Depended on by:** every crate's test target

---

## Purpose

`ft-testkit` provides the test infrastructure every other crate consumes. Isolated
test repositories, record factories, assertion helpers, and the scenario runner
skeleton. Without `ft-testkit`, every crate's integration tests would reinvent the
same fixtures.

It is a regular library crate, not a `dev-dependencies`-only crate, because the
scenario runner is also used by `tests/scenarios/` at workspace level.

---

## Public API

### TestRepo

```rust
/// An isolated Firetrail workspace backed by a tempdir. Drops automatically clean up.
pub struct TestRepo {
    root: PathBuf,
    _tempdir: tempfile::TempDir,  // RAII cleanup
}

impl TestRepo {
    /// Create a fresh empty repo with a git init and a minimal .firetrail/.
    pub fn new() -> Result<Self, TestKitError>;

    /// Create a repo with a custom config (used to test alternate scope or storage modes).
    pub fn with_config(config: TestRepoConfig) -> Result<Self, TestKitError>;

    /// Absolute path of the repo root.
    pub fn root(&self) -> &Path;

    /// Absolute path of .firetrail/.
    pub fn firetrail_dir(&self) -> PathBuf;

    /// Commit currently staged changes.
    pub fn commit(&self, message: &str) -> Result<(), TestKitError>;

    /// Stage everything under the repo and commit.
    pub fn commit_all(&self, message: &str) -> Result<(), TestKitError>;

    /// Create a branch from current HEAD.
    pub fn branch(&self, name: &str) -> Result<(), TestKitError>;

    /// Check out a branch.
    pub fn checkout(&self, name: &str) -> Result<(), TestKitError>;

    /// Current branch name.
    pub fn current_branch(&self) -> Result<String, TestKitError>;

    /// Run a shell command in the repo root. Returns stdout, stderr, exit code.
    pub fn run(&self, cmd: &str, args: &[&str]) -> Result<CmdOutput, TestKitError>;

    /// Run the firetrail binary against this repo (locates the binary via CARGO_BIN_EXE).
    pub fn firetrail(&self, args: &[&str]) -> Result<CmdOutput, TestKitError>;
}

pub struct TestRepoConfig {
    pub storage_mode: StorageMode,            // Embedded | External(url)
    pub strict_identity: bool,
    pub author_email: String,
    pub author_name: String,
    pub install_hooks: bool,
}

pub struct CmdOutput {
    pub stdout: String,
    pub stderr: String,
    pub status: ExitStatus,
}
```

### Record factories

Builder-style factories that produce valid records with sensible defaults. Each
factory accepts overrides via fluent methods.

```rust
pub fn make_task() -> TaskBuilder;
pub fn make_epic() -> EpicBuilder;
pub fn make_subtask(parent: RecordId) -> SubtaskBuilder;
pub fn make_bug() -> BugBuilder;
pub fn make_identity() -> Identity;        // returns a deterministic test identity
pub fn make_identity_named(name: &str) -> Identity;
```

Builders share a common shape:

```rust
pub struct TaskBuilder {
    title: String,
    description: String,
    status: Status,
    priority: Priority,
    owner: Option<Identity>,
    parent_epic: Option<RecordId>,
    acceptance_criteria: Vec<AcceptanceCriterion>,
    labels: Vec<Label>,
    owning_scope: Option<String>,
}

impl TaskBuilder {
    pub fn title(self, t: impl Into<String>) -> Self;
    pub fn description(self, d: impl Into<String>) -> Self;
    pub fn status(self, s: Status) -> Self;
    pub fn priority(self, p: Priority) -> Self;
    pub fn owner(self, o: Identity) -> Self;
    pub fn parent_epic(self, id: RecordId) -> Self;
    pub fn acceptance_criterion(self, text: impl Into<String>) -> Self;
    pub fn label(self, key: impl Into<String>, value: impl Into<String>) -> Self;
    pub fn owning_scope(self, scope: impl Into<String>) -> Self;
    pub fn build(self) -> Record;
}
```

Default values: title `"test task"`, status `Open`, priority `P2`, owner `None`,
acceptance criteria empty, labels empty.

### Assertion helpers

```rust
/// Assert a record file exists at the expected path.
pub fn assert_record_exists(repo: &TestRepo, id: &RecordId);

/// Assert a record's field matches the expected value.
pub fn assert_field<T: serde::de::DeserializeOwned + PartialEq + std::fmt::Debug>(
    repo: &TestRepo, id: &RecordId, field: &str, expected: T,
);

/// Assert a relation exists between two records.
pub fn assert_relation(repo: &TestRepo, from: &RecordId, to: &RecordId, kind: RelationKind);

/// Assert the JSON file's canonical hash matches state_hash.
pub fn assert_hash_consistent(repo: &TestRepo, id: &RecordId);

/// Pretty-print the workspace for debugging in test failures.
pub fn dump_workspace(repo: &TestRepo);
```

### Scenario runner

The runner reads a scenario file, executes its steps against a `TestRepo`, and
asserts expected outcomes.

```rust
pub struct ScenarioRunner;

impl ScenarioRunner {
    pub fn run(scenario_path: &Path) -> Result<ScenarioReport, ScenarioError>;
    pub fn run_str(scenario: &str) -> Result<ScenarioReport, ScenarioError>;
}

pub struct ScenarioReport {
    pub name: String,
    pub steps_run: usize,
    pub steps_passed: usize,
    pub failures: Vec<ScenarioFailure>,
    pub elapsed: Duration,
}

pub struct ScenarioFailure {
    pub step_index: usize,
    pub step_description: String,
    pub message: String,
    pub workspace_dump: Option<String>,  // captured at failure for debugging
}
```

#### Scenario file format

YAML for human readability. Example:

```yaml
name: m1-happy-path
description: Init, create epic + tasks with deps, close in order, board reflects state.

setup:
  config:
    storage_mode: embedded
    strict_identity: false
    author_email: alice@example.com
    author_name: Alice

steps:
  - name: init repo
    run: firetrail init
    expect:
      exit: 0
      stdout_contains: "Firetrail initialized"

  - name: create epic
    run: firetrail epic create "Improve checkout" --description "..."
    expect:
      exit: 0
    capture:
      epic_id: stdout_field=id

  - name: create task with parent epic
    run: firetrail task create "Add Redis alert" --epic ${epic_id}
    expect:
      exit: 0
    capture:
      task_id: stdout_field=id

  - name: assert task is ready
    run: firetrail ready --json
    expect:
      exit: 0
      stdout_json_contains:
        - id: ${task_id}

  - name: close task fails without ACs
    run: firetrail close ${task_id}
    expect:
      exit: 1
      stderr_contains: "acceptance criteria"
```

Capture variables (`${var}`) substitute in later steps. `stdout_field=id` extracts
`id` from a JSON object printed to stdout.

### Mock embedder (used from M3)

```rust
/// Embeds inputs deterministically by hashing — useful for testing search ranking
/// without loading an ONNX model.
pub struct MockEmbedder { seed: u64 }

impl MockEmbedder {
    pub fn new(seed: u64) -> Self;
    pub fn embed(&self, text: &str) -> Vec<f32>;
}
```

At M1, this is a stub returning `vec![]`; M3 fills in the deterministic embedding.

### Errors

```rust
#[derive(Debug, thiserror::Error)]
pub enum TestKitError {
    #[error("io error: {0}")] Io(#[from] std::io::Error),
    #[error("git error: {0}")] Git(String),
    #[error("command failed: {0}")] Cmd(String),
    #[error("invalid config: {0}")] Config(String),
}

#[derive(Debug, thiserror::Error)]
pub enum ScenarioError {
    #[error("parse error: {0}")] Parse(String),
    #[error("step failed: {0}")] StepFailed(String),
    #[error("setup failed: {0}")] Setup(String),
    #[error(transparent)] TestKit(#[from] TestKitError),
}
```

---

## Internal design

### Tempdir cleanup

`TestRepo` owns a `tempfile::TempDir`. When the `TestRepo` is dropped, the tempdir
is removed. Tests must not retain references to paths beyond the `TestRepo`'s
lifetime.

### Git initialization

`TestRepo::new` runs `git init`, sets `user.email` and `user.name` from config,
and makes an initial empty commit so subsequent commits have an ancestor.

### Locating the firetrail binary

`TestRepo::firetrail` uses `env!("CARGO_BIN_EXE_firetrail")` to find the built
binary. Tests that need the binary depend on `ft-cli` to ensure it is built first.

For unit tests in crates that do not build the binary, `TestRepo` is still usable
via `run` for git operations.

---

## Acceptance

1. `TestRepo::new()` produces an isolated workspace; two instances do not share
   state.
2. Tempdir cleanup runs on `Drop`. A test that creates 100 `TestRepo` instances
   leaves at most a small set behind in `/tmp` (cleanup is not perfectly
   deterministic on panics).
3. Factories produce records that round-trip through `serde_json` and pass
   `ft-core` validation.
4. A trivial scenario file (create a task, assert it exists) runs successfully
   end-to-end.
5. Assertion helpers produce useful diagnostics on failure (the expected and actual
   values are both printed; the workspace is dumped).
6. `ScenarioRunner` parses the YAML format above and executes all step kinds
   (`run`, `expect`, `capture`).
7. `ft-testkit::TestRepo` is consumed by `ft-core`'s integration tests as a sanity
   check.

---

## Testing requirements

- Unit tests for each builder default value.
- Property tests for record factories: any combination of overrides produces a
  valid record.
- Integration test: create `TestRepo`, write a record via `ft-storage`, read it
  back, assert content. (Cross-crate; placed in `ft-testkit/tests/`.)
- Scenario runner test against the trivial scenario from acceptance criterion 4.

---

## Out of scope

- Full scenario library — that is E-M1-10.
- Network mocking (no networked operations at M1).
- Concurrency stress testing (M3+).

---

## References

- ADR-0016 — Build approach (test harness layers)

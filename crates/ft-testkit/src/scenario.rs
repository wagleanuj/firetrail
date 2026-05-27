//! Scenario runner.
//!
//! Parses a YAML scenario file (see `docs/components/ft-testkit.md` for the
//! format), executes its steps against a fresh [`TestRepo`], and produces a
//! [`ScenarioReport`].
//!
//! Step kinds supported: `run`, `expect`, `capture`, plus per-step `env:` and
//! `cwd:` overrides. The runner can invoke:
//!
//! - The `firetrail` binary, if a path is supplied via
//!   [`RunnerOptions::firetrail_bin`] (or the compile-time
//!   `CARGO_BIN_EXE_firetrail` env var when ft-cli is a sibling dep). Because
//!   ft-testkit must not depend on ft-cli (cycle), the integration test that
//!   drives the M1 suite passes the path explicitly via [`RunnerOptions`].
//! - Arbitrary shell commands (e.g. `git`, `rm`), useful for setup steps.
//! - `testkit:` virtual commands kept as a **legacy fallback** for the
//!   runner's own self-tests (`testkit:create-task`, `testkit:create-epic`,
//!   `testkit:assert-exists`). ft-testkit cannot depend on ft-cli (workspace
//!   cycle), so the runner needs an in-process way to exercise itself without
//!   a real `firetrail` binary path. New scenarios should prefer the real
//!   `firetrail …` command form — pass the binary path via
//!   [`RunnerOptions::firetrail_bin`]. The `testkit:` shortcuts are scheduled
//!   for removal once a stable in-tree binary fixture is available
//!   (`firetrail-xrr` resolved this in interim mode; canonical removal will
//!   ride alongside the next ft-testkit refresh).
//!
//! ## Capture expressions
//!
//! - `stdout_field=foo`           — top-level JSON key on stdout.
//! - `stdout_field=foo.bar.baz`   — dotted path into nested JSON on stdout.
//! - `stderr_field=foo.bar`       — same, against stderr.
//! - `stdout`                     — raw stdout, trimmed.
//! - `stderr`                     — raw stderr, trimmed.
//!
//! ## Expect block
//!
//! ```yaml
//! expect:
//!   exit: 0
//!   stdout_contains: "some substring"
//!   stderr_contains: "another substring"
//!   stdout_json_path:
//!     data.record.envelope.status: "closed"
//!   stderr_json_path:
//!     error.code: 1
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use serde::Deserialize;

use crate::assertions::{assert_record_exists, dump_workspace_string, write_record};
use crate::error::{ScenarioError, ScenarioFailure, ScenarioReport};
use crate::factories::{make_epic, make_task};
use crate::repo::{StorageMode, TestRepo, TestRepoConfig};

// ---------------------------------------------------------------------------
// YAML schema
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ScenarioDoc {
    name: String,
    #[serde(default)]
    #[allow(dead_code)]
    description: Option<String>,
    #[serde(default)]
    setup: Option<SetupDoc>,
    steps: Vec<StepDoc>,
}

#[derive(Debug, Deserialize)]
struct SetupDoc {
    #[serde(default)]
    config: Option<ConfigDoc>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct ConfigDoc {
    storage_mode: Option<String>,
    strict_identity: Option<bool>,
    author_email: Option<String>,
    author_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StepDoc {
    name: String,
    run: String,
    #[serde(default)]
    expect: Option<ExpectDoc>,
    #[serde(default)]
    capture: Option<HashMap<String, String>>,
    /// Per-step environment variables to add to the spawned process.
    #[serde(default)]
    env: HashMap<String, String>,
    /// Working directory relative to the repo root. Default: repo root.
    #[serde(default)]
    cwd: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct ExpectDoc {
    exit: Option<i32>,
    stdout_contains: Option<String>,
    stderr_contains: Option<String>,
    /// Dotted JSON paths in stdout that must equal the given value.
    stdout_json_path: Option<HashMap<String, serde_yaml::Value>>,
    /// Dotted JSON paths in stderr that must equal the given value.
    stderr_json_path: Option<HashMap<String, serde_yaml::Value>>,
}

// ---------------------------------------------------------------------------
// Runner options
// ---------------------------------------------------------------------------

/// Options injected into the runner by the calling test target.
#[derive(Debug, Default, Clone)]
pub struct RunnerOptions {
    /// Explicit path to the `firetrail` binary. When `None`, the runner falls
    /// back to `CARGO_BIN_EXE_firetrail` resolved at ft-testkit's compile time
    /// (only set when ft-cli is a sibling dep of the test target).
    pub firetrail_bin: Option<PathBuf>,
    /// Environment variables applied to every spawned process. Per-step `env:`
    /// entries override these.
    pub env: HashMap<String, String>,
}

impl RunnerOptions {
    /// Convenience: set the firetrail binary path.
    #[must_use]
    pub fn with_firetrail_bin(mut self, path: impl Into<PathBuf>) -> Self {
        self.firetrail_bin = Some(path.into());
        self
    }

    /// Convenience: set an env var applied to every spawned process.
    #[must_use]
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }
}

// ---------------------------------------------------------------------------
// Runner
// ---------------------------------------------------------------------------

/// Driver for YAML scenario files.
#[derive(Debug)]
pub struct ScenarioRunner;

impl ScenarioRunner {
    /// Run a scenario from a file path with default options.
    ///
    /// # Errors
    ///
    /// See [`ScenarioError`].
    pub fn run(scenario_path: &Path) -> Result<ScenarioReport, ScenarioError> {
        Self::run_with_options(scenario_path, &RunnerOptions::default())
    }

    /// Run a scenario from a YAML string with default options.
    ///
    /// # Errors
    ///
    /// See [`ScenarioError`].
    pub fn run_str(scenario: &str) -> Result<ScenarioReport, ScenarioError> {
        Self::run_str_with_options(scenario, &RunnerOptions::default())
    }

    /// Run a scenario from a file path, with explicit runner options.
    ///
    /// # Errors
    ///
    /// See [`ScenarioError`].
    pub fn run_with_options(
        scenario_path: &Path,
        opts: &RunnerOptions,
    ) -> Result<ScenarioReport, ScenarioError> {
        let text = std::fs::read_to_string(scenario_path)?;
        Self::run_str_with_options(&text, opts)
    }

    /// Run a scenario from a YAML string, with explicit runner options.
    ///
    /// # Errors
    ///
    /// See [`ScenarioError`].
    pub fn run_str_with_options(
        scenario: &str,
        opts: &RunnerOptions,
    ) -> Result<ScenarioReport, ScenarioError> {
        let doc: ScenarioDoc = serde_yaml::from_str(scenario)?;
        let start = Instant::now();

        let config = build_config(doc.setup.as_ref())?;
        let repo = TestRepo::with_config(config)
            .map_err(|e| ScenarioError::Setup(format!("TestRepo::with_config: {e}")))?;

        let mut vars: HashMap<String, String> = HashMap::new();
        let mut failures: Vec<ScenarioFailure> = Vec::new();
        let mut passed = 0usize;

        for (i, step) in doc.steps.iter().enumerate() {
            match run_step(&repo, step, &mut vars, opts) {
                Ok(()) => passed += 1,
                Err(message) => failures.push(ScenarioFailure {
                    step_index: i,
                    step_description: step.name.clone(),
                    message,
                    workspace_dump: Some(dump_workspace_string(&repo)),
                }),
            }
        }

        Ok(ScenarioReport {
            name: doc.name,
            steps_run: doc.steps.len(),
            steps_passed: passed,
            failures,
            elapsed: start.elapsed(),
        })
    }
}

fn build_config(setup: Option<&SetupDoc>) -> Result<TestRepoConfig, ScenarioError> {
    let mut cfg = TestRepoConfig::default();
    let Some(setup) = setup else { return Ok(cfg) };
    let Some(c) = setup.config.as_ref() else {
        return Ok(cfg);
    };
    if let Some(mode) = c.storage_mode.as_deref() {
        cfg.storage_mode = match mode {
            "embedded" => StorageMode::Embedded,
            other if other.starts_with("external:") => {
                StorageMode::External(other.trim_start_matches("external:").to_string())
            }
            other => {
                return Err(ScenarioError::Setup(format!(
                    "unknown storage_mode `{other}`"
                )));
            }
        };
    }
    if let Some(v) = c.strict_identity {
        cfg.strict_identity = v;
    }
    if let Some(v) = c.author_email.clone() {
        cfg.author_email = v;
    }
    if let Some(v) = c.author_name.clone() {
        cfg.author_name = v;
    }
    Ok(cfg)
}

fn run_step(
    repo: &TestRepo,
    step: &StepDoc,
    vars: &mut HashMap<String, String>,
    opts: &RunnerOptions,
) -> Result<(), String> {
    let cmd = substitute_vars(&step.run, vars);
    let argv = shell_split(&cmd).map_err(|e| format!("parse step.run: {e}"))?;

    let cwd = resolve_cwd(repo, step.cwd.as_deref(), vars)?;
    let mut env = opts.env.clone();
    for (k, v) in &step.env {
        env.insert(k.clone(), substitute_vars(v, vars));
    }

    let output = dispatch(repo, &argv, &cwd, &env, opts)
        .map_err(|e| format!("step `{}` execution failed: {e}", step.name))?;

    if let Some(exp) = step.expect.as_ref() {
        check_expect(exp, &output, vars)?;
    }

    if let Some(cap) = step.capture.as_ref() {
        for (var, expr) in cap {
            let value = capture_value(expr, &output)
                .map_err(|e| format!("capture `{var}` from `{expr}`: {e}"))?;
            vars.insert(var.clone(), value);
        }
    }

    Ok(())
}

fn resolve_cwd(
    repo: &TestRepo,
    cwd: Option<&str>,
    vars: &HashMap<String, String>,
) -> Result<PathBuf, String> {
    let Some(rel) = cwd else {
        return Ok(repo.root().to_path_buf());
    };
    let rel = substitute_vars(rel, vars);
    let joined = repo.root().join(&rel);
    if !joined.exists() {
        return Err(format!(
            "step cwd `{}` does not exist under repo root",
            joined.display()
        ));
    }
    Ok(joined)
}

fn check_expect(
    exp: &ExpectDoc,
    output: &StepOutput,
    vars: &HashMap<String, String>,
) -> Result<(), String> {
    if let Some(want_exit) = exp.exit {
        let got = output.exit.unwrap_or(-1);
        if got != want_exit {
            return Err(format!(
                "exit code mismatch: expected {want_exit}, got {got}\n  stdout = {}\n  \
                 stderr = {}",
                output.stdout, output.stderr
            ));
        }
    }
    if let Some(s) = exp.stdout_contains.as_deref() {
        let needle = substitute_vars(s, vars);
        if !output.stdout.contains(needle.as_str()) {
            return Err(format!(
                "stdout does not contain `{needle}`\n  stdout = {}",
                output.stdout
            ));
        }
    }
    if let Some(s) = exp.stderr_contains.as_deref() {
        let needle = substitute_vars(s, vars);
        if !output.stderr.contains(needle.as_str()) {
            return Err(format!(
                "stderr does not contain `{needle}`\n  stderr = {}",
                output.stderr
            ));
        }
    }
    if let Some(paths) = exp.stdout_json_path.as_ref() {
        check_json_paths("stdout", &output.stdout, paths, vars)?;
    }
    if let Some(paths) = exp.stderr_json_path.as_ref() {
        check_json_paths("stderr", &output.stderr, paths, vars)?;
    }
    Ok(())
}

fn check_json_paths(
    stream: &str,
    raw: &str,
    paths: &HashMap<String, serde_yaml::Value>,
    vars: &HashMap<String, String>,
) -> Result<(), String> {
    let trimmed = raw.trim();
    let v: serde_json::Value = serde_json::from_str(trimmed)
        .map_err(|e| format!("{stream} is not JSON: {e}\n  {stream} = {raw}"))?;
    for (path, expected) in paths {
        let resolved_path = substitute_vars(path, vars);
        let got = json_at_path(&v, &resolved_path).ok_or_else(|| {
            format!("{stream} json path `{resolved_path}` missing\n  {stream} = {raw}")
        })?;
        let mut expected_json: serde_json::Value =
            serde_json::to_value(expected).map_err(|e| format!("invalid expected value: {e}"))?;
        substitute_in_json(&mut expected_json, vars);
        if got != &expected_json {
            return Err(format!(
                "{stream} json path `{resolved_path}` mismatch: expected {expected_json}, got {got}"
            ));
        }
    }
    Ok(())
}

fn substitute_in_json(v: &mut serde_json::Value, vars: &HashMap<String, String>) {
    match v {
        serde_json::Value::String(s) => {
            *s = substitute_vars(s, vars);
        }
        serde_json::Value::Array(arr) => {
            for item in arr {
                substitute_in_json(item, vars);
            }
        }
        serde_json::Value::Object(obj) => {
            for val in obj.values_mut() {
                substitute_in_json(val, vars);
            }
        }
        _ => {}
    }
}

fn json_at_path<'a>(v: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    let mut cur = v;
    for seg in path.split('.') {
        // Support numeric indices for arrays.
        if let Ok(idx) = seg.parse::<usize>() {
            cur = cur.get(idx)?;
        } else {
            cur = cur.get(seg)?;
        }
    }
    Some(cur)
}

/// Output abstraction across `firetrail` binary and `testkit:` commands.
struct StepOutput {
    stdout: String,
    stderr: String,
    exit: Option<i32>,
}

fn dispatch(
    repo: &TestRepo,
    argv: &[String],
    cwd: &Path,
    env: &HashMap<String, String>,
    opts: &RunnerOptions,
) -> Result<StepOutput, String> {
    let Some(head) = argv.first() else {
        return Err("empty command".to_string());
    };

    if let Some(sub) = head.strip_prefix("testkit:") {
        return dispatch_testkit(repo, sub, &argv[1..]);
    }

    if head == "firetrail" {
        let bin = resolve_firetrail_bin(opts)?;
        let str_args: Vec<&str> = argv[1..].iter().map(String::as_str).collect();
        return spawn(&bin, &str_args, cwd, env);
    }

    let str_args: Vec<&str> = argv[1..].iter().map(String::as_str).collect();
    spawn(head, &str_args, cwd, env)
}

fn resolve_firetrail_bin(opts: &RunnerOptions) -> Result<String, String> {
    if let Some(p) = opts.firetrail_bin.as_ref() {
        return Ok(p.display().to_string());
    }
    if let Some(p) = option_env!("CARGO_BIN_EXE_firetrail") {
        return Ok(p.to_string());
    }
    Err(
        "firetrail binary unavailable: pass RunnerOptions::firetrail_bin or compile from a \
         crate that has ft-cli as a sibling dep"
            .to_string(),
    )
}

fn spawn(
    cmd: &str,
    args: &[&str],
    cwd: &Path,
    env: &HashMap<String, String>,
) -> Result<StepOutput, String> {
    let mut c = Command::new(cmd);
    c.args(args).current_dir(cwd);
    for (k, v) in env {
        c.env(k, v);
    }
    let out = c
        .output()
        .map_err(|e| format!("spawn `{cmd}` failed: {e}"))?;
    Ok(StepOutput {
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        exit: out.status.code(),
    })
}

/// Legacy in-process dispatch of `testkit:` virtual commands.
///
/// Kept for ft-testkit's own self-test scenarios (notably `trivial.yml`)
/// because ft-testkit may not depend on ft-cli. New scenarios should call
/// `firetrail …` directly via [`RunnerOptions::firetrail_bin`].
fn dispatch_testkit(repo: &TestRepo, sub: &str, args: &[String]) -> Result<StepOutput, String> {
    match sub {
        "create-task" => {
            let title = arg_value(args, "--title").unwrap_or("test task");
            let record = make_task().title(title).build();
            write_record(repo, &record).map_err(|e| e.to_string())?;
            Ok(StepOutput {
                stdout: format!("{{\"id\":\"{}\"}}\n", record.envelope.id),
                stderr: String::new(),
                exit: Some(0),
            })
        }
        "create-epic" => {
            let title = arg_value(args, "--title").unwrap_or("test epic");
            let record = make_epic().title(title).build();
            write_record(repo, &record).map_err(|e| e.to_string())?;
            Ok(StepOutput {
                stdout: format!("{{\"id\":\"{}\"}}\n", record.envelope.id),
                stderr: String::new(),
                exit: Some(0),
            })
        }
        "assert-exists" => {
            let id = arg_value(args, "--id").ok_or("missing --id")?;
            let parsed =
                ft_core::RecordId::from_string(id.to_string()).map_err(|e| e.to_string())?;
            assert_record_exists(repo, &parsed);
            Ok(StepOutput {
                stdout: format!("ok {id}\n"),
                stderr: String::new(),
                exit: Some(0),
            })
        }
        other => Err(format!("unknown testkit command `{other}`")),
    }
}

fn arg_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    let mut iter = args.iter();
    while let Some(a) = iter.next() {
        if a == flag {
            return iter.next().map(String::as_str);
        }
    }
    None
}

fn capture_value(expr: &str, out: &StepOutput) -> Result<String, String> {
    if let Some(field) = expr.strip_prefix("stdout_field=") {
        return extract_field(&out.stdout, field, "stdout");
    }
    if let Some(field) = expr.strip_prefix("stderr_field=") {
        return extract_field(&out.stderr, field, "stderr");
    }
    if expr == "stdout" {
        return Ok(out.stdout.trim().to_string());
    }
    if expr == "stderr" {
        return Ok(out.stderr.trim().to_string());
    }
    Err(format!("unsupported capture expression `{expr}`"))
}

fn extract_field(raw: &str, path: &str, stream: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    let v: serde_json::Value =
        serde_json::from_str(trimmed).map_err(|e| format!("{stream} is not JSON: {e}"))?;
    let got = json_at_path(&v, path)
        .ok_or_else(|| format!("field `{path}` missing in {stream}: {trimmed}"))?;
    Ok(match got {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    })
}

fn substitute_vars(input: &str, vars: &HashMap<String, String>) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
            if let Some(end) = input[i + 2..].find('}') {
                let key = &input[i + 2..i + 2 + end];
                if let Some(v) = vars.get(key) {
                    out.push_str(v);
                } else {
                    out.push_str(&input[i..i + 3 + end]);
                }
                i += 3 + end;
                continue;
            }
        }
        out.push(input[i..].chars().next().unwrap());
        i += input[i..].chars().next().unwrap().len_utf8();
    }
    out
}

/// Minimal shell splitter: handles single/double quotes, no escapes beyond
/// backslash-quote inside double quotes. Sufficient for scenario syntax.
fn shell_split(input: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut chars = input.chars().peekable();
    let mut in_single = false;
    let mut in_double = false;
    let mut has_token = false;

    while let Some(c) = chars.next() {
        match c {
            '\'' if !in_double => {
                in_single = !in_single;
                has_token = true;
            }
            '"' if !in_single => {
                in_double = !in_double;
                has_token = true;
            }
            '\\' if in_double => {
                if let Some(&next) = chars.peek() {
                    current.push(next);
                    chars.next();
                } else {
                    return Err("trailing backslash".into());
                }
            }
            c if c.is_whitespace() && !in_single && !in_double => {
                if has_token {
                    out.push(std::mem::take(&mut current));
                    has_token = false;
                }
            }
            c => {
                current.push(c);
                has_token = true;
            }
        }
    }
    if in_single || in_double {
        return Err("unterminated quote".into());
    }
    if has_token {
        out.push(current);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_split_basic() {
        assert_eq!(
            shell_split("a b c").unwrap(),
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
        assert_eq!(
            shell_split(r#"foo --title "hello world""#).unwrap(),
            vec![
                "foo".to_string(),
                "--title".to_string(),
                "hello world".to_string()
            ]
        );
    }

    #[test]
    fn substitute_vars_replaces_known_keys() {
        let mut vars = HashMap::new();
        vars.insert("epic_id".to_string(), "EPIC-abc".to_string());
        assert_eq!(
            substitute_vars("--epic ${epic_id} --x", &vars),
            "--epic EPIC-abc --x"
        );
    }

    #[test]
    fn substitute_vars_leaves_unknown_keys() {
        let vars = HashMap::new();
        assert_eq!(substitute_vars("${nope}", &vars), "${nope}");
    }

    #[test]
    fn json_at_path_walks_dotted_segments() {
        let v: serde_json::Value =
            serde_json::from_str(r#"{"data":{"record":{"envelope":{"id":"TASK-1"}}}}"#).unwrap();
        let got = json_at_path(&v, "data.record.envelope.id").unwrap();
        assert_eq!(got, &serde_json::json!("TASK-1"));
    }

    #[test]
    fn json_at_path_supports_array_indices() {
        let v: serde_json::Value =
            serde_json::from_str(r#"{"rows":[{"id":"A"},{"id":"B"}]}"#).unwrap();
        let got = json_at_path(&v, "rows.1.id").unwrap();
        assert_eq!(got, &serde_json::json!("B"));
    }
}

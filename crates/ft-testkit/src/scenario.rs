//! Scenario runner skeleton.
//!
//! Parses a YAML scenario file (see `docs/components/ft-testkit.md` for the
//! format), executes its steps against a fresh [`TestRepo`], and produces a
//! [`ScenarioReport`]. The full scenario library lives in E-M1-10; this
//! skeleton supports the step kinds documented in the spec (`run`/`expect`/
//! `capture`) and is enough for the trivial scenario in acceptance criterion
//! #4.
//!
//! ## Built-in commands
//!
//! In addition to invoking the `firetrail` binary (which is a stub until
//! ft-cli lands), the runner understands a `testkit:` virtual command family
//! used for the bootstrap scenario:
//!
//! ```text
//! testkit:create-task --title "..."   -> writes a Task record; prints {"id": "..."}
//! testkit:create-epic --title "..."   -> writes an Epic record
//! testkit:assert-exists --id "..."    -> asserts the record file exists
//! ```
//!
//! These commands let scenarios exercise the runner machinery before the CLI
//! is wired. The on-disk shape they produce mirrors what ft-storage will
//! eventually own.

use std::collections::HashMap;
use std::path::Path;
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
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct ExpectDoc {
    exit: Option<i32>,
    stdout_contains: Option<String>,
    stderr_contains: Option<String>,
}

// ---------------------------------------------------------------------------
// Runner
// ---------------------------------------------------------------------------

/// Driver for YAML scenario files.
#[derive(Debug)]
pub struct ScenarioRunner;

impl ScenarioRunner {
    /// Run a scenario from a file path.
    ///
    /// # Errors
    ///
    /// See [`ScenarioError`].
    pub fn run(scenario_path: &Path) -> Result<ScenarioReport, ScenarioError> {
        let text = std::fs::read_to_string(scenario_path)?;
        Self::run_str(&text)
    }

    /// Run a scenario from a YAML string.
    ///
    /// # Errors
    ///
    /// See [`ScenarioError`].
    pub fn run_str(scenario: &str) -> Result<ScenarioReport, ScenarioError> {
        let doc: ScenarioDoc = serde_yaml::from_str(scenario)?;
        let start = Instant::now();

        let config = build_config(doc.setup.as_ref())?;
        let repo = TestRepo::with_config(config)
            .map_err(|e| ScenarioError::Setup(format!("TestRepo::with_config: {e}")))?;

        let mut vars: HashMap<String, String> = HashMap::new();
        let mut failures: Vec<ScenarioFailure> = Vec::new();
        let mut passed = 0usize;

        for (i, step) in doc.steps.iter().enumerate() {
            match run_step(&repo, step, &mut vars) {
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
) -> Result<(), String> {
    let cmd = substitute_vars(&step.run, vars);
    let argv = shell_split(&cmd).map_err(|e| format!("parse step.run: {e}"))?;

    let output =
        dispatch(repo, &argv).map_err(|e| format!("step `{}` execution failed: {e}", step.name))?;

    if let Some(exp) = step.expect.as_ref() {
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
            if !output.stdout.contains(s) {
                return Err(format!(
                    "stdout does not contain `{s}`\n  stdout = {}",
                    output.stdout
                ));
            }
        }
        if let Some(s) = exp.stderr_contains.as_deref() {
            if !output.stderr.contains(s) {
                return Err(format!(
                    "stderr does not contain `{s}`\n  stderr = {}",
                    output.stderr
                ));
            }
        }
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

/// Output abstraction across `firetrail` binary and `testkit:` commands.
struct StepOutput {
    stdout: String,
    stderr: String,
    exit: Option<i32>,
}

fn dispatch(repo: &TestRepo, argv: &[String]) -> Result<StepOutput, String> {
    let Some(head) = argv.first() else {
        return Err("empty command".to_string());
    };

    if let Some(sub) = head.strip_prefix("testkit:") {
        return dispatch_testkit(repo, sub, &argv[1..]);
    }

    if head == "firetrail" {
        let str_args: Vec<&str> = argv[1..].iter().map(String::as_str).collect();
        return match repo.firetrail(&str_args) {
            Ok(o) => Ok(StepOutput {
                stdout: o.stdout,
                stderr: o.stderr,
                exit: o.status.code(),
            }),
            Err(e) => Err(format!("firetrail binary unavailable: {e}")),
        };
    }

    // Generic shell command.
    let str_args: Vec<&str> = argv[1..].iter().map(String::as_str).collect();
    match repo.run(head, &str_args) {
        Ok(o) => Ok(StepOutput {
            stdout: o.stdout,
            stderr: o.stderr,
            exit: o.status.code(),
        }),
        Err(e) => Err(e.to_string()),
    }
}

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
        let v: serde_json::Value = serde_json::from_str(out.stdout.trim())
            .map_err(|e| format!("stdout is not JSON: {e}"))?;
        let got = v
            .get(field)
            .ok_or_else(|| format!("field `{field}` missing"))?;
        return Ok(match got {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        });
    }
    if expr == "stdout" {
        return Ok(out.stdout.trim().to_string());
    }
    Err(format!("unsupported capture expression `{expr}`"))
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
}

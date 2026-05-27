//! # ft-identity
//!
//! Identity resolution for Firetrail. Given a workspace root and an
//! environment, return the canonical [`Identity`] that should be stamped on
//! the next record write.
//!
//! M1 ships the resolution path only — the registry, capabilities, kinds,
//! on-behalf-of, and offboarding sweep described in ADR-0008 are deferred to
//! M5. The trait shape is forward compatible with the M5 additions.
//!
//! ## Resolution order
//!
//! 1. `$FIRETRAIL_AUTHOR` environment variable.
//! 2. `.firetrail/identity.yml` — the `name` or `email` field.
//! 3. `.firetrail/config.yml` — the `identity.name` (or `identity.email`) field.
//! 4. `git config user.email` (and `user.name` as fallback) read by shelling
//!    out to `git` from the workspace root.
//!
//! If every source declines (no value, not invalid), an
//! [`IdentityError::Unresolved`] is returned naming every source that was
//! consulted.
//!
//! ## Relevant ADRs
//!
//! - ADR-0008 — Identity registry
//! - ADR-0013 — Trust model
//!
//! ## Example
//!
//! ```
//! use ft_identity::{DefaultResolver, IdentityResolver, MockEnv};
//!
//! let env = MockEnv::new().with("FIRETRAIL_AUTHOR", "alice@example.com");
//! let resolver = DefaultResolver::with_env(
//!     std::env::temp_dir(),
//!     false,
//!     Box::new(env),
//! );
//! let identity = resolver.resolve().unwrap();
//! assert_eq!(identity.as_str(), "alice@example.com");
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use ft_core::{CoreError, Identity};
use serde::Deserialize;
use thiserror::Error;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors returned by identity resolution.
#[derive(Debug, Error)]
pub enum IdentityError {
    /// No source produced a value.
    #[error("no identity resolvable from any source; checked: {0}")]
    Unresolved(String),

    /// Strict mode rejected the resolved value (M1: never fired, reserved for M5).
    #[error("strict mode rejected identity '{0}': {1}")]
    StrictRejection(String, String),

    /// A source produced a value that failed validation.
    #[error("invalid identity value '{value}' from {from:?}: {reason}")]
    Invalid {
        /// The raw value that failed validation.
        value: String,
        /// Which resolution source produced the value.
        ///
        /// Field is named `from` (not `source`) so that `thiserror`'s `Error`
        /// derive does not treat it as a wrapped error cause.
        from: ResolutionSource,
        /// Human readable validation failure.
        reason: String,
    },

    /// I/O failure while reading a config file or running `git`.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    /// `ft-core` rejected the validated value when constructing [`Identity`].
    #[error("core: {0}")]
    Core(#[from] CoreError),
}

// ---------------------------------------------------------------------------
// Trace types
// ---------------------------------------------------------------------------

/// Diagnostic record describing what each resolution source returned.
///
/// Surfaces via [`IdentityResolver::resolve_with_trace`] for `firetrail doctor`
/// and verbose CLI output.
#[derive(Debug, Clone)]
pub struct ResolutionTrace {
    /// The successfully resolved identity, if any.
    pub resolved_identity: Option<Identity>,
    /// Per-source outcomes, in the order they were consulted.
    pub sources_checked: Vec<SourceCheck>,
    /// Whether the resolver was running in strict mode.
    pub strict_mode: bool,
}

/// A single resolution source and its outcome.
#[derive(Debug, Clone)]
pub struct SourceCheck {
    /// Which source was consulted.
    pub source: ResolutionSource,
    /// What it returned.
    pub result: SourceResult,
}

/// The four sources consulted by [`DefaultResolver`] in priority order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionSource {
    /// `FIRETRAIL_AUTHOR` environment variable.
    EnvVar,
    /// `.firetrail/identity.yml` — `name` / `email` field.
    LocalIdentityFile,
    /// `.firetrail/config.yml` — `identity.name` / `identity.email` field.
    LocalConfig,
    /// `git config user.email` (with `user.name` fallback).
    GitConfig,
}

impl ResolutionSource {
    /// Human readable label, used in [`IdentityError::Unresolved`] messages.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::EnvVar => "$FIRETRAIL_AUTHOR",
            Self::LocalIdentityFile => ".firetrail/identity.yml",
            Self::LocalConfig => ".firetrail/config.yml",
            Self::GitConfig => "git config user.email",
        }
    }
}

/// Outcome of consulting a single resolution source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceResult {
    /// The source produced this candidate identity string.
    Found(String),
    /// The source had no opinion (env unset, file missing, field absent).
    NotPresent,
    /// The source produced a value, but it failed validation.
    Invalid(String),
}

// ---------------------------------------------------------------------------
// EnvSource
// ---------------------------------------------------------------------------

/// Abstraction over process environment access.
///
/// Production code passes [`StdEnv`]; tests pass [`MockEnv`] to exercise
/// resolution order without polluting the test process's real environment.
pub trait EnvSource: Send + Sync {
    /// Look up an environment variable.
    fn get(&self, key: &str) -> Option<String>;
}

/// Real-process environment source. Wraps [`std::env::var`].
#[derive(Debug, Default, Clone, Copy)]
pub struct StdEnv;

impl EnvSource for StdEnv {
    fn get(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }
}

/// In-memory environment, for tests.
///
/// # Examples
///
/// ```
/// use ft_identity::{EnvSource, MockEnv};
///
/// let env = MockEnv::new().with("FIRETRAIL_AUTHOR", "alice@example.com");
/// assert_eq!(env.get("FIRETRAIL_AUTHOR").as_deref(), Some("alice@example.com"));
/// assert!(env.get("MISSING").is_none());
/// ```
#[derive(Debug, Default, Clone)]
pub struct MockEnv {
    vars: HashMap<String, String>,
}

impl MockEnv {
    /// Empty mock environment.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Builder helper: insert a variable.
    #[must_use]
    pub fn with(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.vars.insert(key.into(), value.into());
        self
    }

    /// Insert or overwrite a variable.
    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.vars.insert(key.into(), value.into());
    }
}

impl EnvSource for MockEnv {
    fn get(&self, key: &str) -> Option<String> {
        self.vars.get(key).cloned()
    }
}

// ---------------------------------------------------------------------------
// IdentityResolver trait
// ---------------------------------------------------------------------------

/// Resolves the current actor.
///
/// Implementors consult one or more sources and return either an
/// [`Identity`] suitable for stamping on a record, or a structured
/// [`IdentityError`].
pub trait IdentityResolver: Send + Sync {
    /// Resolve the current actor.
    ///
    /// # Errors
    ///
    /// Returns [`IdentityError::Unresolved`] when no source produces a value,
    /// or [`IdentityError::Invalid`] when a source produces an unusable value.
    fn resolve(&self) -> Result<Identity, IdentityError>;

    /// Resolve with a full diagnostic trace.
    ///
    /// Even on success the trace reports every source consulted up to and
    /// including the one that succeeded. On failure the trace reports every
    /// source.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`Self::resolve`] when the resolution
    /// outcome itself is fatal. I/O errors reading a config file still
    /// propagate via [`IdentityError::Io`].
    fn resolve_with_trace(&self) -> Result<ResolutionTrace, IdentityError>;
}

// ---------------------------------------------------------------------------
// Identity validation
// ---------------------------------------------------------------------------

/// Maximum byte length of a valid identity value (RFC 5321 / 5322 ceiling).
pub const IDENTITY_MAX_LEN: usize = 254;

/// Validate a candidate identity string per M1 rules:
///
/// - Trimmed length is 1..=254 characters.
/// - No whitespace, no control characters.
/// - Either contains `@` (email-shaped) or matches `[a-zA-Z0-9._-]+` (token).
///
/// Returns the trimmed canonical form on success.
///
/// # Errors
///
/// Returns a human-readable reason string on failure. The caller wraps it in
/// [`IdentityError::Invalid`] together with the source that produced the value.
///
/// # Examples
///
/// ```
/// use ft_identity::validate_identity_value;
///
/// assert!(validate_identity_value("alice@example.com").is_ok());
/// assert!(validate_identity_value("ci-runner_01").is_ok());
/// assert!(validate_identity_value("").is_err());
/// assert!(validate_identity_value("alice with space").is_err());
/// ```
pub fn validate_identity_value(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("identity is empty".to_string());
    }
    if trimmed.len() > IDENTITY_MAX_LEN {
        return Err(format!(
            "identity length {} exceeds maximum {}",
            trimmed.len(),
            IDENTITY_MAX_LEN
        ));
    }
    for ch in trimmed.chars() {
        if ch.is_whitespace() {
            return Err("identity contains whitespace".to_string());
        }
        if ch.is_control() {
            return Err("identity contains control character".to_string());
        }
    }

    let is_email_shaped = trimmed.contains('@');
    let is_token_shaped = trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));

    if !is_email_shaped && !is_token_shaped {
        return Err(
            "identity must contain '@' (email) or match [a-zA-Z0-9._-]+ (token)".to_string(),
        );
    }

    Ok(trimmed.to_string())
}

// ---------------------------------------------------------------------------
// DefaultResolver
// ---------------------------------------------------------------------------

/// Environment variable consulted by [`DefaultResolver`].
pub const ENV_VAR_NAME: &str = "FIRETRAIL_AUTHOR";

/// Default resolver: env var, then local config files, then git config.
///
/// Construct with [`DefaultResolver::new`] for normal use, or
/// [`DefaultResolver::with_env`] to inject a [`MockEnv`] in tests.
pub struct DefaultResolver {
    workspace_root: PathBuf,
    env: Box<dyn EnvSource>,
    strict_mode: bool,
}

impl std::fmt::Debug for DefaultResolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DefaultResolver")
            .field("workspace_root", &self.workspace_root)
            .field("strict_mode", &self.strict_mode)
            .finish_non_exhaustive()
    }
}

impl DefaultResolver {
    /// Build a resolver backed by the real process environment.
    pub fn new(workspace_root: impl Into<PathBuf>, strict: bool) -> Self {
        Self {
            workspace_root: workspace_root.into(),
            env: Box::new(StdEnv),
            strict_mode: strict,
        }
    }

    /// Build a resolver with an explicit [`EnvSource`]. Used by tests.
    pub fn with_env(
        workspace_root: impl Into<PathBuf>,
        strict: bool,
        env: Box<dyn EnvSource>,
    ) -> Self {
        Self {
            workspace_root: workspace_root.into(),
            env,
            strict_mode: strict,
        }
    }

    /// Workspace root.
    #[must_use]
    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    /// Whether the resolver was constructed in strict mode.
    ///
    /// In M1 this is a no-op (no registry exists yet to enforce against).
    /// In M5 the field is wired to the identity registry.
    #[must_use]
    pub fn strict_mode(&self) -> bool {
        self.strict_mode
    }
}

impl IdentityResolver for DefaultResolver {
    fn resolve(&self) -> Result<Identity, IdentityError> {
        let trace = self.resolve_with_trace()?;
        trace
            .resolved_identity
            .ok_or_else(|| IdentityError::Unresolved(unresolved_message(&trace.sources_checked)))
    }

    fn resolve_with_trace(&self) -> Result<ResolutionTrace, IdentityError> {
        let mut sources_checked: Vec<SourceCheck> = Vec::with_capacity(4);
        let mut resolved: Option<Identity> = None;

        // 1. Environment variable.
        let env_outcome = check_env(self.env.as_ref());
        let env_done = matches!(env_outcome.result, SourceResult::Found(_));
        let env_invalid = matches!(env_outcome.result, SourceResult::Invalid(_));
        let env_value = match &env_outcome.result {
            SourceResult::Found(v) => Some(v.clone()),
            _ => None,
        };
        sources_checked.push(env_outcome);

        if env_invalid {
            // Fail fast on explicit but malformed env var; the caller asked.
            return Err(invalid_from_check(
                &sources_checked,
                ResolutionSource::EnvVar,
            ));
        }
        if env_done {
            let value = env_value.expect("Found implies Some");
            resolved = Some(Identity::new(value)?);
        }

        // 2. .firetrail/identity.yml
        if resolved.is_none() {
            let check = check_identity_file(&self.workspace_root)?;
            let outcome = check.result.clone();
            sources_checked.push(check);
            if let SourceResult::Found(v) = outcome {
                resolved = Some(Identity::new(v)?);
            } else if let SourceResult::Invalid(_) = outcome {
                return Err(invalid_from_check(
                    &sources_checked,
                    ResolutionSource::LocalIdentityFile,
                ));
            }
        } else {
            sources_checked.push(SourceCheck {
                source: ResolutionSource::LocalIdentityFile,
                result: SourceResult::NotPresent,
            });
        }

        // 3. .firetrail/config.yml
        if resolved.is_none() {
            let check = check_local_config(&self.workspace_root)?;
            let outcome = check.result.clone();
            sources_checked.push(check);
            if let SourceResult::Found(v) = outcome {
                resolved = Some(Identity::new(v)?);
            } else if let SourceResult::Invalid(_) = outcome {
                return Err(invalid_from_check(
                    &sources_checked,
                    ResolutionSource::LocalConfig,
                ));
            }
        } else {
            sources_checked.push(SourceCheck {
                source: ResolutionSource::LocalConfig,
                result: SourceResult::NotPresent,
            });
        }

        // 4. git config user.email
        if resolved.is_none() {
            let check = check_git_config(&self.workspace_root, self.env.as_ref());
            let outcome = check.result.clone();
            sources_checked.push(check);
            if let SourceResult::Found(v) = outcome {
                resolved = Some(Identity::new(v)?);
            } else if let SourceResult::Invalid(_) = outcome {
                return Err(invalid_from_check(
                    &sources_checked,
                    ResolutionSource::GitConfig,
                ));
            }
        } else {
            sources_checked.push(SourceCheck {
                source: ResolutionSource::GitConfig,
                result: SourceResult::NotPresent,
            });
        }

        Ok(ResolutionTrace {
            resolved_identity: resolved,
            sources_checked,
            strict_mode: self.strict_mode,
        })
    }
}

// ---------------------------------------------------------------------------
// Source implementations
// ---------------------------------------------------------------------------

fn check_env(env: &dyn EnvSource) -> SourceCheck {
    let result = match env.get(ENV_VAR_NAME) {
        None => SourceResult::NotPresent,
        Some(raw) => match validate_identity_value(&raw) {
            Ok(v) => SourceResult::Found(v),
            Err(_reason) => SourceResult::Invalid(raw),
        },
    };
    SourceCheck {
        source: ResolutionSource::EnvVar,
        result,
    }
}

/// Schema of `.firetrail/identity.yml` (the M1 form).
///
/// Only the `name` and `email` fields are read in M1; everything else (the
/// `identities:` registry block from ADR-0008) is ignored until M5.
#[derive(Debug, Default, Deserialize)]
struct IdentityFile {
    name: Option<String>,
    email: Option<String>,
}

fn check_identity_file(root: &Path) -> Result<SourceCheck, IdentityError> {
    let path = root.join(".firetrail").join("identity.yml");
    let result = read_yaml_field(&path, |s| {
        let parsed: IdentityFile = serde_yaml::from_str(s).map_err(|e| e.to_string())?;
        // Prefer `email`, fall back to `name`. M5 will resolve through the
        // registry; M1 treats either field as the canonical value.
        Ok(parsed.email.or(parsed.name))
    });
    Ok(SourceCheck {
        source: ResolutionSource::LocalIdentityFile,
        result: result?,
    })
}

#[derive(Debug, Default, Deserialize)]
struct LocalConfig {
    identity: Option<LocalConfigIdentity>,
}

#[derive(Debug, Default, Deserialize)]
struct LocalConfigIdentity {
    name: Option<String>,
    email: Option<String>,
}

fn check_local_config(root: &Path) -> Result<SourceCheck, IdentityError> {
    let path = root.join(".firetrail").join("config.yml");
    let result = read_yaml_field(&path, |s| {
        let parsed: LocalConfig = serde_yaml::from_str(s).map_err(|e| e.to_string())?;
        Ok(parsed.identity.and_then(|i| i.email.or(i.name)))
    });
    Ok(SourceCheck {
        source: ResolutionSource::LocalConfig,
        result: result?,
    })
}

/// Read a YAML file and extract a candidate identity string via `extract`.
///
/// Returns:
/// - `NotPresent` if the file is missing or the field is absent.
/// - `Found(canonical)` if the extracted value passes validation.
/// - `Invalid(raw)` if the file exists, contains a value, but the value or
///   the YAML itself failed to parse / validate.
fn read_yaml_field<F>(path: &Path, extract: F) -> Result<SourceResult, IdentityError>
where
    F: FnOnce(&str) -> Result<Option<String>, String>,
{
    let contents = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(SourceResult::NotPresent),
        Err(e) => return Err(IdentityError::Io(e)),
    };

    match extract(&contents) {
        Ok(None) => Ok(SourceResult::NotPresent),
        Ok(Some(raw)) => match validate_identity_value(&raw) {
            Ok(canonical) => Ok(SourceResult::Found(canonical)),
            Err(_) => Ok(SourceResult::Invalid(raw)),
        },
        Err(_yaml_err) => Ok(SourceResult::Invalid(contents)),
    }
}

fn check_git_config(root: &Path, env: &dyn EnvSource) -> SourceCheck {
    let email = git_config_value(root, "user.email", env);
    let result = match email {
        Some(raw) if !raw.trim().is_empty() => match validate_identity_value(&raw) {
            Ok(v) => SourceResult::Found(v),
            Err(_) => SourceResult::Invalid(raw),
        },
        _ => {
            // Fall back to `user.name` (token-shaped) if email is absent.
            match git_config_value(root, "user.name", env) {
                Some(raw) if !raw.trim().is_empty() => match validate_identity_value(&raw) {
                    Ok(v) => SourceResult::Found(v),
                    Err(_) => SourceResult::Invalid(raw),
                },
                _ => SourceResult::NotPresent,
            }
        }
    };
    SourceCheck {
        source: ResolutionSource::GitConfig,
        result,
    }
}

/// Run `git config --get <key>` in `root`, returning the trimmed value if any.
///
/// Git's config scopes (system / global / local) are honored normally. Two
/// well-known overrides are forwarded from the [`EnvSource`] when present —
/// `GIT_CONFIG_GLOBAL` and `GIT_CONFIG_SYSTEM` — so callers (most notably
/// tests) can pin git to read only a known scope without polluting the host
/// process environment.
fn git_config_value(root: &Path, key: &str, env: &dyn EnvSource) -> Option<String> {
    let mut cmd = Command::new("git");
    cmd.args(["config", "--get", key]).current_dir(root);
    for var in [
        "GIT_CONFIG_GLOBAL",
        "GIT_CONFIG_SYSTEM",
        "GIT_CONFIG_NOSYSTEM",
    ] {
        if let Some(v) = env.get(var) {
            cmd.env(var, v);
        }
    }
    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if value.is_empty() { None } else { Some(value) }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn unresolved_message(checks: &[SourceCheck]) -> String {
    let parts: Vec<String> = checks
        .iter()
        .map(|c| format!("{} ({})", c.source.label(), describe_result(&c.result)))
        .collect();
    parts.join(", ")
}

fn describe_result(r: &SourceResult) -> &'static str {
    match r {
        SourceResult::Found(_) => "found",
        SourceResult::NotPresent => "not present",
        SourceResult::Invalid(_) => "invalid",
    }
}

fn invalid_from_check(checks: &[SourceCheck], source: ResolutionSource) -> IdentityError {
    let value = checks
        .iter()
        .rev()
        .find_map(|c| {
            if c.source == source {
                match &c.result {
                    SourceResult::Invalid(v) | SourceResult::Found(v) => Some(v.clone()),
                    SourceResult::NotPresent => None,
                }
            } else {
                None
            }
        })
        .unwrap_or_default();
    let reason = validate_identity_value(&value)
        .err()
        .unwrap_or_else(|| "failed source-specific validation".to_string());
    IdentityError::Invalid {
        value,
        from: source,
        reason,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn workspace(tmp: &TempDir) -> PathBuf {
        let root = tmp.path().to_path_buf();
        std::fs::create_dir_all(root.join(".firetrail")).unwrap();
        root
    }

    /// Build a [`MockEnv`] that disables ambient git config so unit tests
    /// don't accidentally pick up the developer's real `user.email`.
    ///
    /// Production callers don't do this — they want ambient git config
    /// resolution. Tests that want to assert "git config is empty" must
    /// neutralize the host environment.
    fn isolated_env() -> MockEnv {
        MockEnv::new()
            .with("GIT_CONFIG_GLOBAL", "/dev/null")
            .with("GIT_CONFIG_SYSTEM", "/dev/null")
            .with("GIT_CONFIG_NOSYSTEM", "1")
    }

    // ---- validate_identity_value --------------------------------------------------

    #[test]
    fn validate_accepts_email() {
        assert_eq!(
            validate_identity_value("alice@example.com").unwrap(),
            "alice@example.com"
        );
    }

    #[test]
    fn validate_accepts_token() {
        assert_eq!(
            validate_identity_value("ci-runner_01").unwrap(),
            "ci-runner_01"
        );
    }

    #[test]
    fn validate_trims_surrounding_whitespace() {
        assert_eq!(
            validate_identity_value("  alice@example.com\n").unwrap(),
            "alice@example.com"
        );
    }

    #[test]
    fn validate_rejects_empty() {
        assert!(validate_identity_value("").is_err());
        assert!(validate_identity_value("   ").is_err());
    }

    #[test]
    fn validate_rejects_internal_whitespace() {
        assert!(validate_identity_value("alice smith@x.com").is_err());
        assert!(validate_identity_value("a\tb").is_err());
    }

    #[test]
    fn validate_rejects_control_chars() {
        assert!(validate_identity_value("a\u{0007}b").is_err());
    }

    #[test]
    fn validate_rejects_overlong() {
        let s = "a".repeat(IDENTITY_MAX_LEN + 1);
        assert!(validate_identity_value(&s).is_err());
    }

    #[test]
    fn validate_rejects_non_email_non_token() {
        assert!(validate_identity_value("alice!").is_err());
        assert!(validate_identity_value("hello/world").is_err());
    }

    // ---- env var ------------------------------------------------------------------

    #[test]
    fn resolves_from_env_var() {
        let tmp = TempDir::new().unwrap();
        let root = workspace(&tmp);
        let env = MockEnv::new().with("FIRETRAIL_AUTHOR", "alice@example.com");
        let r = DefaultResolver::with_env(root, false, Box::new(env));
        let id = r.resolve().unwrap();
        assert_eq!(id.as_str(), "alice@example.com");
    }

    #[test]
    fn env_var_invalid_returns_invalid_error() {
        let tmp = TempDir::new().unwrap();
        let root = workspace(&tmp);
        let env = MockEnv::new().with("FIRETRAIL_AUTHOR", "alice with space@x.com");
        let r = DefaultResolver::with_env(root, false, Box::new(env));
        let err = r.resolve().unwrap_err();
        assert!(
            matches!(
                err,
                IdentityError::Invalid {
                    from: ResolutionSource::EnvVar,
                    ..
                }
            ),
            "got: {err:?}"
        );
    }

    #[test]
    fn env_var_empty_returns_invalid_error() {
        let tmp = TempDir::new().unwrap();
        let root = workspace(&tmp);
        let env = MockEnv::new().with("FIRETRAIL_AUTHOR", "");
        let r = DefaultResolver::with_env(root, false, Box::new(env));
        let err = r.resolve().unwrap_err();
        assert!(matches!(
            err,
            IdentityError::Invalid {
                from: ResolutionSource::EnvVar,
                ..
            }
        ));
    }

    // ---- .firetrail/identity.yml --------------------------------------------------

    #[test]
    fn resolves_from_identity_yml_email_field() {
        let tmp = TempDir::new().unwrap();
        let root = workspace(&tmp);
        std::fs::write(
            root.join(".firetrail/identity.yml"),
            "email: alice@example.com\n",
        )
        .unwrap();
        let env = MockEnv::new();
        let r = DefaultResolver::with_env(root, false, Box::new(env));
        assert_eq!(r.resolve().unwrap().as_str(), "alice@example.com");
    }

    #[test]
    fn resolves_from_identity_yml_name_field() {
        let tmp = TempDir::new().unwrap();
        let root = workspace(&tmp);
        std::fs::write(root.join(".firetrail/identity.yml"), "name: ci-runner_01\n").unwrap();
        let env = MockEnv::new();
        let r = DefaultResolver::with_env(root, false, Box::new(env));
        assert_eq!(r.resolve().unwrap().as_str(), "ci-runner_01");
    }

    #[test]
    fn identity_yml_email_overrides_name() {
        let tmp = TempDir::new().unwrap();
        let root = workspace(&tmp);
        std::fs::write(
            root.join(".firetrail/identity.yml"),
            "name: bob\nemail: alice@example.com\n",
        )
        .unwrap();
        let r = DefaultResolver::with_env(root, false, Box::new(isolated_env()));
        assert_eq!(r.resolve().unwrap().as_str(), "alice@example.com");
    }

    // ---- .firetrail/config.yml ----------------------------------------------------

    #[test]
    fn resolves_from_config_yml_identity_name() {
        let tmp = TempDir::new().unwrap();
        let root = workspace(&tmp);
        std::fs::write(
            root.join(".firetrail/config.yml"),
            "identity:\n  name: alice@example.com\n",
        )
        .unwrap();
        let r = DefaultResolver::with_env(root, false, Box::new(isolated_env()));
        assert_eq!(r.resolve().unwrap().as_str(), "alice@example.com");
    }

    #[test]
    fn identity_yml_takes_precedence_over_config_yml() {
        let tmp = TempDir::new().unwrap();
        let root = workspace(&tmp);
        std::fs::write(
            root.join(".firetrail/identity.yml"),
            "email: from-identity@example.com\n",
        )
        .unwrap();
        std::fs::write(
            root.join(".firetrail/config.yml"),
            "identity:\n  email: from-config@example.com\n",
        )
        .unwrap();
        let r = DefaultResolver::with_env(root, false, Box::new(isolated_env()));
        assert_eq!(r.resolve().unwrap().as_str(), "from-identity@example.com");
    }

    // ---- env var precedence over files --------------------------------------------

    #[test]
    fn env_var_takes_precedence_over_files() {
        let tmp = TempDir::new().unwrap();
        let root = workspace(&tmp);
        std::fs::write(
            root.join(".firetrail/identity.yml"),
            "email: from-file@example.com\n",
        )
        .unwrap();
        let env = MockEnv::new().with("FIRETRAIL_AUTHOR", "from-env@example.com");
        let r = DefaultResolver::with_env(root, false, Box::new(env));
        assert_eq!(r.resolve().unwrap().as_str(), "from-env@example.com");
    }

    // ---- unresolved ---------------------------------------------------------------

    #[test]
    fn unresolved_when_no_source_present() {
        let tmp = TempDir::new().unwrap();
        let root = workspace(&tmp);
        // Disable ambient git config (developer's ~/.gitconfig) so this test
        // is portable across hosts.
        let r = DefaultResolver::with_env(root, false, Box::new(isolated_env()));
        let err = r.resolve().unwrap_err();
        match err {
            IdentityError::Unresolved(msg) => {
                assert!(msg.contains("$FIRETRAIL_AUTHOR"));
                assert!(msg.contains("identity.yml"));
                assert!(msg.contains("config.yml"));
                assert!(msg.contains("git config"));
            }
            other => panic!("expected Unresolved, got {other:?}"),
        }
    }

    // ---- trace --------------------------------------------------------------------

    #[test]
    fn trace_reports_every_source_on_failure() {
        let tmp = TempDir::new().unwrap();
        let root = workspace(&tmp);
        let r = DefaultResolver::with_env(root, false, Box::new(isolated_env()));
        let trace = r.resolve_with_trace().unwrap();
        assert!(trace.resolved_identity.is_none());
        assert_eq!(trace.sources_checked.len(), 4);
        assert_eq!(trace.sources_checked[0].source, ResolutionSource::EnvVar);
        assert_eq!(
            trace.sources_checked[1].source,
            ResolutionSource::LocalIdentityFile
        );
        assert_eq!(
            trace.sources_checked[2].source,
            ResolutionSource::LocalConfig
        );
        assert_eq!(trace.sources_checked[3].source, ResolutionSource::GitConfig);
    }

    #[test]
    fn trace_marks_succeeding_source_found() {
        let tmp = TempDir::new().unwrap();
        let root = workspace(&tmp);
        let env = MockEnv::new().with("FIRETRAIL_AUTHOR", "alice@example.com");
        let r = DefaultResolver::with_env(root, false, Box::new(env));
        let trace = r.resolve_with_trace().unwrap();
        assert!(trace.resolved_identity.is_some());
        assert!(matches!(
            trace.sources_checked[0].result,
            SourceResult::Found(_)
        ));
        // Later sources are short-circuited to NotPresent.
        assert!(matches!(
            trace.sources_checked[1].result,
            SourceResult::NotPresent
        ));
    }

    #[test]
    fn strict_mode_flag_recorded_in_trace() {
        let tmp = TempDir::new().unwrap();
        let root = workspace(&tmp);
        let env = MockEnv::new().with("FIRETRAIL_AUTHOR", "alice@example.com");
        let r = DefaultResolver::with_env(root, true, Box::new(env));
        let trace = r.resolve_with_trace().unwrap();
        assert!(trace.strict_mode);
        // M1: strict mode does not reject valid values.
        assert!(trace.resolved_identity.is_some());
    }

    // ---- malformed yaml -----------------------------------------------------------

    #[test]
    fn identity_yml_malformed_yields_invalid() {
        let tmp = TempDir::new().unwrap();
        let root = workspace(&tmp);
        std::fs::write(root.join(".firetrail/identity.yml"), "::: not yaml :::\n").unwrap();
        let r = DefaultResolver::with_env(root, false, Box::new(isolated_env()));
        let err = r.resolve().unwrap_err();
        assert!(matches!(
            err,
            IdentityError::Invalid {
                from: ResolutionSource::LocalIdentityFile,
                ..
            }
        ));
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn email_shaped_with_no_whitespace_validates(local in "[a-z0-9._-]{1,32}", domain in "[a-z0-9.-]{1,32}") {
            let candidate = format!("{local}@{domain}");
            prop_assert!(validate_identity_value(&candidate).is_ok(),
                "expected ok for {candidate}");
        }

        #[test]
        fn token_shaped_validates(token in "[a-zA-Z0-9._-]{1,64}") {
            prop_assert!(validate_identity_value(&token).is_ok());
        }

        #[test]
        fn strings_with_whitespace_always_fail(left in "[a-z]{1,8}", right in "[a-z]{1,8}") {
            let candidate = format!("{left} {right}");
            prop_assert!(validate_identity_value(&candidate).is_err());
        }
    }
}

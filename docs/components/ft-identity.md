# ft-identity — actor identity resolution (M1 form)

**Epic:** `firetrail-c4c`
**Wave:** 2
**Depends on:** ft-core
**Depended on by:** ft-cli, ft-storage, ft-pr (M4), ft-trust (M2)

---

## Purpose

`ft-identity` resolves "who is performing this operation" into a canonical
`Identity`. Every record write attributes to a resolved identity; the resolution
process is centralized here.

M1 ships the resolution path only — the registry, capabilities, kinds, on-behalf-of,
and offboarding sweep (ADR-0008) are added in M5. The trait shape is forward-
compatible with the M5 additions.

---

## Public API

```rust
pub trait IdentityResolver: Send + Sync {
    /// Resolve the current actor. The result is suitable for stamping on a record.
    fn resolve(&self) -> Result<Identity, IdentityError>;

    /// Resolution diagnostics — which sources were checked, what each returned.
    /// Used by `firetrail doctor` and verbose output.
    fn resolve_with_trace(&self) -> Result<ResolutionTrace, IdentityError>;
}

pub struct DefaultResolver {
    workspace_root: PathBuf,
    env: Box<dyn EnvSource>,
    strict_mode: bool,
}

impl DefaultResolver {
    pub fn new(workspace_root: impl Into<PathBuf>, strict: bool) -> Self;

    /// Construct with a custom env source for testing.
    pub fn with_env(workspace_root: impl Into<PathBuf>, strict: bool,
                    env: Box<dyn EnvSource>) -> Self;
}

impl IdentityResolver for DefaultResolver { /* ... */ }

/// Abstraction over environment access. Production uses std::env; tests inject
/// a mock to exercise resolution order without polluting the test process env.
pub trait EnvSource: Send + Sync {
    fn get(&self, key: &str) -> Option<String>;
}

pub struct StdEnv;
impl EnvSource for StdEnv {
    fn get(&self, key: &str) -> Option<String> { std::env::var(key).ok() }
}

pub struct ResolutionTrace {
    pub resolved_identity: Option<Identity>,
    pub sources_checked: Vec<SourceCheck>,
    pub strict_mode: bool,
}

pub struct SourceCheck {
    pub source: ResolutionSource,
    pub result: SourceResult,
}

pub enum ResolutionSource {
    EnvVar,                     // FIRETRAIL_AUTHOR
    LocalConfig,                // .firetrail/identity.yml or config.yml identity.name
    GitConfig,                  // git config user.email + user.name
}

pub enum SourceResult {
    Found(String),
    NotPresent,
    Invalid(String),
}
```

### Errors

```rust
#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    #[error("no identity resolvable from any source; checked: {0}")]
    Unresolved(String),

    #[error("strict mode rejected identity '{0}': {1}")]
    StrictRejection(String, String),

    #[error("invalid identity value '{value}' from {source:?}: {reason}")]
    Invalid { value: String, source: ResolutionSource, reason: String },

    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("core: {0}")]
    Core(#[from] CoreError),
}
```

---

## Resolution order (M1)

```
1. Read $FIRETRAIL_AUTHOR
   - If set and valid → resolve to Identity, return.
   - If set and invalid → return Invalid error.
   - If unset → continue.

2. Read .firetrail/identity.yml (if present) for the `name` or `email` field.
   - If present and valid → resolve, return.
   - If absent → continue.

3. Read .firetrail/config.yml `identity.name` (legacy alternative location).
   - Same logic.

4. Read git config user.email + user.name via ft-git's StatusReport.
   - If found and valid → resolve, return.

5. None found → return IdentityError::Unresolved with a trace of all sources.
```

In strict mode (configured per workspace), step 5 fires immediately when the
resolved identity is not in a known-identities list. The M1 implementation
treats strict mode as a no-op (no registry yet exists). M5 wires it to the
registry.

---

## Identity format (M1)

A valid `Identity` at M1 is a non-empty string that looks like an email address
or a sortable token. Validation rules:

- Length 1–254 characters.
- Contains either `@` (email) or matches `^[a-zA-Z0-9._-]+$` (token form for
  bots / CI runners).
- No whitespace, no control characters.

The canonical form is the trimmed string preserving case. Comparisons are
case-sensitive at M1 (M5 may add case-folding for known aliases).

---

## Acceptance

1. Resolution from env var: setting `FIRETRAIL_AUTHOR=alice@example.com` returns
   that identity regardless of git config or local files.
2. Resolution from local config: writing `.firetrail/identity.yml` with
   `name: alice@example.com` resolves when env var is unset.
3. Resolution from git config: setting `git config user.email alice@example.com`
   resolves when env var and local config are unset.
4. Resolution order is observable via `resolve_with_trace`: every source is
   reported with `Found`, `NotPresent`, or `Invalid`.
5. Unresolved case: with no env var, no local config, and no git config,
   `resolve` returns `IdentityError::Unresolved` with a clear message naming all
   four sources checked.
6. Invalid identity case: `FIRETRAIL_AUTHOR=`(empty) and
   `FIRETRAIL_AUTHOR="alice with space@x.com"` both fail with `Invalid`.
7. Strict mode in M1: the field exists in `DefaultResolver` and `ResolutionTrace`
   but does not enforce anything beyond the basic validation (registry comes
   in M5).
8. Mocked env: tests use `EnvSource` to avoid polluting the test process env.

---

## Testing requirements

- Unit tests for each resolution source independently using `EnvSource` mocks.
- Property tests for `Identity` validation: random strings either pass or fail
  predictably; the validation matches the rules above.
- Integration test via `ft-testkit::TestRepo`: configure git, run `resolve`,
  assert correct identity.
- Doc tests on every public method.

---

## Out of scope (deferred)

- Identity registry with aliases (ADR-0008, M5).
- Capability matrix (`kind: human|bot|ci`, what each can do) — M5.
- On-behalf-of resolution for CI runners — M5.
- Offboarding sweep and claim takeover — M5.
- Co-authorship via `Co-authored-by` trailer — M5.
- External contributor handling — M5.

The `Identity` type itself does not change in M5; only the resolver gains new
capabilities. Code written against `IdentityResolver` at M1 continues to work in
M5 unchanged.

---

## References

- ADR-0008 — Identity registry (full spec; M5 implementation)
- ADR-0013 — Trust model (depends on resolved identity for review attribution)

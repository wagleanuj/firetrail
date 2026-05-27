# workspace-skeleton — Cargo workspace + tooling

**Epic:** `firetrail-bnp`
**Wave:** 0 (foundation; blocks every other M1 epic)
**Crates touched:** all of them, as empty skeletons

---

## Purpose

Stand up the Cargo workspace, scaffold every crate, configure shared tooling, and
install the validation gates. This is the foundation everything else builds on.

This deliverable is mostly configuration and conventions. No business logic.

---

## Deliverables

### Workspace root

```
Cargo.toml          workspace definition
rustfmt.toml        formatting rules
.clippy.toml        clippy configuration
rust-toolchain.toml stable, 2024 edition
deny.toml           cargo-deny config (licenses, security advisories)
.cargo/config.toml  build settings (mold linker on linux, etc.)
```

### Crate skeletons under `crates/`

Empty crates with `Cargo.toml` and minimal `lib.rs` or `main.rs`:

```
crates/
  ft-core/
  ft-storage/
  ft-index/
  ft-embed/
  ft-identity/
  ft-scope/
  ft-trust/
  ft-history/
  ft-search/
  ft-prime/
  ft-pr/
  ft-import/
  ft-git/
  ft-cli/            (bin crate, binary name: firetrail)
  ft-testkit/
```

Each crate's `Cargo.toml` declares its name, version (`0.0.0`), edition (`2024`),
and inherits shared dependencies from `[workspace.dependencies]`.

### Shared workspace dependencies

Declared in root `Cargo.toml` under `[workspace.dependencies]`:

```
serde, serde_json, serde_yaml      record marshalling
thiserror                          library errors
anyhow                             ft-cli error glue
clap (derive)                      ft-cli command parser
rusqlite (bundled)                 ft-index storage
sqlite-vec                         ft-index vector tables (added in M3)
gix                                ft-git reads
tokio (full)                       ft-embed daemon (added in M3)
proptest                           property testing
insta                              snapshot testing
tempfile                           test fixtures
assert_cmd                         CLI integration tests
ort                                ONNX runtime (added in M3)
jsonschema                         record validation
hex, sha2                          hash chain
chrono                             timestamps
tracing, tracing-subscriber        diagnostics
```

Concrete versions pinned at root; crates depend by name only.

### Scripts and tooling

```
scripts/
  validate.sh              fmt + clippy + nextest --workspace
  install-hooks.sh         writes pre-commit hook running validate.sh fast subset
```

`justfile` (or `Makefile`) with at minimum:

```
default        # run validate.sh
test           # cargo nextest run --workspace
fmt            # cargo fmt --all
lint           # cargo clippy --workspace -- -D warnings
ci             # full validation matching CI
```

### CI workflow

`.github/workflows/ci.yml` running:

1. `cargo fmt --check`
2. `cargo clippy --workspace -- -D warnings`
3. `cargo nextest run --workspace`
4. `cargo test --doc`
5. (placeholder for scenario suite, added in E-M1-10)

Matrix across `ubuntu-latest` and `macos-latest`. Caches Cargo registry and target
directory via `Swatinem/rust-cache`.

### Documentation hooks

Add `cargo doc --workspace --no-deps --document-private-items` to CI (warnings allowed
but logged). Future ADRs may make doc warnings fatal.

---

## Acceptance

A subagent claiming this epic done must demonstrate:

1. `cargo build --workspace` succeeds with zero warnings.
2. `cargo nextest run --workspace` runs successfully against zero tests (exit 0).
3. `cargo clippy --workspace -- -D warnings` passes.
4. `cargo fmt --check` passes.
5. `./scripts/validate.sh` returns exit code 0.
6. CI workflow file is valid YAML and `act` (or equivalent local runner) parses it
   without errors.
7. Pre-commit hook installs via `scripts/install-hooks.sh` and runs the fast subset
   (fmt + clippy on staged files).
8. Each crate's `lib.rs` (or `main.rs` for `ft-cli`) contains at minimum a module-level
   doc comment naming the crate's purpose and the relevant ADRs.

---

## Out of scope for this epic

- Any actual business logic in any crate.
- The `ft-cli` binary's actual command surface (E-M1-08 and E-M1-09).
- The scenario runner (E-M1-02 and E-M1-10).
- Dependency relations between crates beyond `Cargo.toml` declarations — those come in
  the implementing epics.

---

## References

- ADR-0001 — Rust as the implementation language
- ADR-0016 — Build approach (workspace structure, validation gates)

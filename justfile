# Firetrail developer commands.
#
# Install `just` (https://github.com/casey/just) then run `just` for the
# default target, or `just <recipe>` for a specific one.

# Default: run the full local validation gate.
default: ci

# Build the whole workspace.
build:
    cargo build --workspace

# Run all tests via cargo-nextest (falls back to cargo test).
test:
    @if command -v cargo-nextest >/dev/null 2>&1; then \
        cargo nextest run --workspace; \
    else \
        cargo test --workspace; \
    fi

# Run doctests separately (nextest does not run them).
doctest:
    cargo test --doc --workspace

# Format every crate.
fmt:
    cargo fmt --all

# Check formatting without rewriting files.
fmt-check:
    cargo fmt --all --check

# Lint with clippy at -D warnings, matching CI.
lint:
    cargo clippy --workspace --all-targets -- -D warnings

# Build docs (no deps, include private items) for local browsing.
doc:
    cargo doc --workspace --no-deps --document-private-items

# Full validation gate. Same as `./scripts/validate.sh`.
ci:
    ./scripts/validate.sh

# Install the git pre-commit hook.
hooks:
    ./scripts/install-hooks.sh

# Clean target/ and incremental caches.
clean:
    cargo clean

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

# Export the ft-core Record JSON Schema to docs/schema/.
schema:
    cargo run -p ft-core --example export_schema --quiet -- docs/schema/firetrail-record-v1.json

# Clean target/ and incremental caches.
clean:
    cargo clean

# --- UI (firetrail GUI) ---

# Run Vite (5173) and ft-ui (5174) concurrently. Ctrl-C stops both.
ui-dev:
    #!/usr/bin/env bash
    set -euo pipefail
    trap 'kill 0' INT TERM
    (cd crates/ft-ui/web && pnpm dev) &
    cargo run -p ft-ui -- --workspace "$(pwd)" --bind 127.0.0.1:5174 --dev --foreground &
    wait

# Build the web bundle and the ft-ui binary with bundled assets.
ui-build:
    pnpm -C crates/ft-ui/web install
    pnpm -C crates/ft-ui/web build
    cargo build -p ft-ui --features bundled-ui --release

# Build + run the production server.
ui:
    just ui-build
    cargo run -p ft-ui --features bundled-ui --release -- --workspace "$(pwd)"

# Regenerate the TypeScript wire types from ft-ops.
ui-gen-ts:
    cargo xtask gen-ts

# Fail if committed TS bindings drift from ft-ops's source of truth.
ui-check-ts:
    cargo xtask check-ts

#!/usr/bin/env bash
# scripts/validate.sh — full local validation gate.
#
# Mirrors what CI runs. Use this before opening a PR; the pre-commit hook
# runs a fast subset of these steps.
#
# Exit codes:
#   0  all checks passed
#   non-zero  some check failed (first failure determines exit code)

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

heading() {
    printf '\n\033[1;34m==> %s\033[0m\n' "$1"
}

heading "cargo fmt --all --check"
cargo fmt --all --check

heading "cargo clippy --workspace --all-targets -- -D warnings"
cargo clippy --workspace --all-targets -- -D warnings

heading "cargo build --workspace"
cargo build --workspace

if command -v cargo-nextest >/dev/null 2>&1; then
    heading "cargo nextest run --workspace"
    cargo nextest run --workspace --no-tests=pass
else
    heading "cargo test --workspace  (cargo-nextest not installed; falling back)"
    cargo test --workspace
fi

heading "cargo test --doc --workspace"
cargo test --doc --workspace

if command -v cargo-deny >/dev/null 2>&1; then
    heading "cargo deny check"
    cargo deny check
else
    printf '\n\033[1;33m==> cargo-deny not installed; skipping supply-chain checks\033[0m\n'
fi

printf '\n\033[1;32mAll validation gates passed.\033[0m\n'

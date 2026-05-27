#!/usr/bin/env bash
# scripts/install-hooks.sh — install git pre-commit hook.
#
# The hook runs the fast subset of validation (fmt + clippy) on staged
# changes before allowing a commit.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
hook_path="$repo_root/.git/hooks/pre-commit"

if [[ ! -d "$repo_root/.git" ]]; then
    echo "error: $repo_root is not a git repository" >&2
    exit 1
fi

mkdir -p "$repo_root/.git/hooks"

cat > "$hook_path" <<'HOOK'
#!/usr/bin/env bash
# Firetrail pre-commit hook — fast validation subset.
#
# Runs cargo fmt --check and cargo clippy. Both are workspace-wide because
# Rust does not cleanly support per-file formatting/lint scoping across a
# workspace. If this becomes too slow, scope to changed crates only.

set -euo pipefail

# Skip if there are no staged Rust or Cargo.toml changes.
if ! git diff --cached --name-only --diff-filter=ACMR \
    | grep -E '\.(rs|toml)$' >/dev/null; then
    exit 0
fi

echo "pre-commit: cargo fmt --all --check"
cargo fmt --all --check

echo "pre-commit: cargo clippy --workspace -- -D warnings"
cargo clippy --workspace --all-targets -- -D warnings
HOOK

chmod +x "$hook_path"
echo "Installed pre-commit hook at $hook_path"

//! CODEOWNERS parser.
//!
//! Implements a small, intentionally-strict subset of the GitHub CODEOWNERS
//! syntax sufficient for Firetrail's M5 needs:
//!
//! - Lines of the form `<pattern> <owner> [<owner> ...]`
//! - `#` introduces a comment; trailing comments on rules are stripped
//! - Blank lines are ignored
//! - Patterns are compiled with [`globset`]; semantics match those used by
//!   [`crate::ScopeRegistry::scopes_for_path`]
//! - The order of returned entries preserves the order of the source file.
//!
//! Notable omissions vs GitHub:
//!
//! - No section headers (`[Optional team]`)
//! - No escape sequences in patterns
//! - No `!`-negation; later-matching rules simply win (callers walk the vec)

use std::path::{Path, PathBuf};

use globset::{Glob, GlobMatcher};

use crate::error::ScopeError;

/// A single parsed line from a CODEOWNERS file: a compiled glob plus the list
/// of literal owner strings declared for it.
#[derive(Debug, Clone)]
pub struct CodeOwnersEntry {
    /// Original source pattern (before compilation), retained for diagnostics.
    pub pattern: String,
    /// Owners declared on the line, in source order. Strings are preserved
    /// verbatim (e.g. `@alice`, `@ops-team`, `alice@example.com`).
    pub owners: Vec<String>,
    /// Compiled matcher for [`Self::pattern`].
    pub matcher: GlobMatcher,
}

impl PartialEq for CodeOwnersEntry {
    fn eq(&self, other: &Self) -> bool {
        self.pattern == other.pattern && self.owners == other.owners
    }
}

impl Eq for CodeOwnersEntry {}

/// Parse the contents of a CODEOWNERS file.
///
/// `source_path` is purely used for diagnostics on glob-compilation errors.
///
/// # Errors
///
/// Returns [`ScopeError::InvalidCodeOwnersGlob`] if any pattern fails to
/// compile.
pub fn parse(source_path: &Path, text: &str) -> Result<Vec<CodeOwnersEntry>, ScopeError> {
    let mut out = Vec::new();
    for raw_line in text.lines() {
        let line = strip_comment(raw_line).trim();
        if line.is_empty() {
            continue;
        }

        let mut parts = line.split_whitespace();
        let Some(pattern) = parts.next() else {
            continue;
        };
        let owners: Vec<String> = parts.map(str::to_owned).collect();
        if owners.is_empty() {
            // A rule with no owners is a no-op; skip rather than error.
            continue;
        }

        let glob = Glob::new(pattern).map_err(|source| ScopeError::InvalidCodeOwnersGlob {
            path: PathBuf::from(source_path),
            pattern: pattern.to_owned(),
            source,
        })?;

        out.push(CodeOwnersEntry {
            pattern: pattern.to_owned(),
            owners,
            matcher: glob.compile_matcher(),
        });
    }
    Ok(out)
}

/// Strip a `#`-introduced trailing comment from a line. `#` inside a token is
/// not honoured because CODEOWNERS patterns cannot legally contain `#`.
fn strip_comment(line: &str) -> &str {
    match line.find('#') {
        Some(idx) => &line[..idx],
        None => line,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignores_comments_and_blanks() {
        let src = "# a header comment\n\n   \n# another\n";
        let entries = parse(Path::new("CODEOWNERS"), src).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn parses_multiple_owners() {
        let src = "apps/checkout/**.rs @alice @bob @ops-team\n";
        let entries = parse(Path::new("CODEOWNERS"), src).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].pattern, "apps/checkout/**.rs");
        assert_eq!(entries[0].owners, vec!["@alice", "@bob", "@ops-team"]);
    }

    #[test]
    fn strips_trailing_comments() {
        let src = "libs/** @core-team   # owns shared libs\n";
        let entries = parse(Path::new("CODEOWNERS"), src).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].owners, vec!["@core-team"]);
    }

    #[test]
    fn skips_rules_without_owners() {
        let entries = parse(Path::new("CODEOWNERS"), "apps/checkout/**\n").unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn rejects_invalid_glob() {
        let err = parse(Path::new("CODEOWNERS"), "[ @alice\n").unwrap_err();
        assert!(matches!(err, ScopeError::InvalidCodeOwnersGlob { .. }));
    }
}

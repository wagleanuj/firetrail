//! Error types for `ft-scope`.

use std::path::PathBuf;

/// Errors produced while loading or operating on a [`crate::ScopeRegistry`].
#[derive(Debug, thiserror::Error)]
pub enum ScopeError {
    /// The scopes file could not be read from disk.
    #[error("failed to read scopes file `{path}`: {source}")]
    Io {
        /// Path of the file that failed to read.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// The scopes file did not parse as valid YAML or did not match the
    /// expected schema.
    #[error("failed to parse scopes file `{path}`: {source}")]
    Yaml {
        /// Path of the file that failed to parse.
        path: PathBuf,
        /// Underlying YAML error.
        #[source]
        source: serde_yaml::Error,
    },

    /// One of the `applies_to` globs failed to compile.
    #[error("invalid glob `{pattern}` for scope `{scope_id}`: {source}")]
    InvalidGlob {
        /// Scope that declared the bad glob.
        scope_id: String,
        /// The pattern that failed to compile.
        pattern: String,
        /// Underlying globset error.
        #[source]
        source: globset::Error,
    },

    /// A CODEOWNERS file referenced by a scope could not be read.
    #[error("failed to load CODEOWNERS `{path}` for scope `{scope_id}`: {source}")]
    CodeOwners {
        /// Scope that referenced the file.
        scope_id: String,
        /// Path to the CODEOWNERS file.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// A CODEOWNERS line contained an invalid glob pattern.
    #[error("invalid CODEOWNERS pattern `{pattern}` in `{path}`: {source}")]
    InvalidCodeOwnersGlob {
        /// Path to the CODEOWNERS file.
        path: PathBuf,
        /// The pattern that failed to compile.
        pattern: String,
        /// Underlying globset error.
        #[source]
        source: globset::Error,
    },

    /// Two scopes share the same alias.
    #[error("alias `{alias}` is claimed by both scope `{first}` and scope `{second}`")]
    DuplicateAlias {
        /// The conflicting alias.
        alias: String,
        /// First scope that claims the alias.
        first: String,
        /// Second scope that claims the alias.
        second: String,
    },

    /// Two scopes declare the same id.
    #[error("duplicate scope id `{id}`")]
    DuplicateScopeId {
        /// The duplicated scope id.
        id: String,
    },

    /// A write operation referenced a scope id that does not exist.
    #[error("scope `{id}` not found")]
    ScopeNotFound {
        /// The id that could not be found.
        id: String,
    },

    /// A scope was written with no `applies_to` patterns. Every scope must
    /// declare at least one pattern.
    #[error("scope `{id}` has no `applies_to` patterns")]
    EmptyAppliesTo {
        /// The scope that is missing patterns.
        id: String,
    },

    /// A reorder request was not a permutation of the existing scope ids.
    #[error("reorder ids are not a permutation of the existing scope ids")]
    ReorderMismatch,
}

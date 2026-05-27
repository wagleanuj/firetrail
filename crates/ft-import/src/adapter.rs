//! External system adapters (Jira / Confluence / GitHub).
//!
//! Real implementations of these adapters live behind MCP servers and will
//! land in follow-up beads issues. This module defines the trait surface and
//! ships a [`MockJiraAdapter`] for testing the import flow without a network.
//!
//! TODO(follow-up): wire the real Jira adapter to the
//! `mcp__plugin_jira_mcp__*` server tools once that integration ships.
//! TODO(follow-up): wire the Confluence adapter to its MCP server analogue.

use std::collections::HashMap;

use crate::error::ImportError;
use crate::source::{ImportSource, SourceSystem};

/// Raw content fetched from an external system, paired with its provenance.
#[derive(Debug, Clone)]
pub struct RawImport {
    /// Where the content came from.
    pub source: ImportSource,
    /// Verbatim content (typically markdown).
    pub content: String,
}

/// A fetcher capable of pulling content out of an external system.
pub trait ImportAdapter {
    /// Fetch a single artefact by its external identifier.
    ///
    /// # Errors
    ///
    /// Implementations return [`ImportError::Adapter`] for any external
    /// failure (network, auth, not-found).
    fn fetch_one(&self, key: &str) -> Result<RawImport, ImportError>;

    /// Fetch a batch of artefacts. Default impl loops over `keys` and calls
    /// [`Self::fetch_one`]; concrete implementations should override to batch
    /// the underlying API calls when possible.
    ///
    /// # Errors
    ///
    /// As for [`Self::fetch_one`].
    fn fetch_batch(&self, keys: &[String]) -> Result<Vec<RawImport>, ImportError> {
        keys.iter().map(|k| self.fetch_one(k)).collect()
    }
}

/// In-memory Jira adapter used by tests.
///
/// Holds a `HashMap<key, content>` and replays it on demand. A real
/// implementation will replace this with calls to the MCP Jira tools.
#[derive(Debug, Default, Clone)]
pub struct MockJiraAdapter {
    /// Mapping of Jira issue key (e.g. `"ENG-123"`) to its markdown content.
    pub issues: HashMap<String, String>,
}

impl MockJiraAdapter {
    /// Construct an empty mock.
    #[must_use]
    pub fn new() -> Self {
        Self {
            issues: HashMap::new(),
        }
    }

    /// Add an issue to the mock store.
    #[must_use]
    pub fn with_issue(mut self, key: impl Into<String>, content: impl Into<String>) -> Self {
        self.issues.insert(key.into(), content.into());
        self
    }
}

impl ImportAdapter for MockJiraAdapter {
    fn fetch_one(&self, key: &str) -> Result<RawImport, ImportError> {
        let content = self
            .issues
            .get(key)
            .ok_or_else(|| ImportError::Adapter(format!("no such jira key: {key}")))?
            .clone();
        Ok(RawImport {
            source: ImportSource {
                url: Some(format!("jira://{key}")),
                file_path: None,
                system: SourceSystem::Jira,
            },
            content,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_adapter_fetch_round_trip() {
        let m = MockJiraAdapter::new().with_issue("ENG-1", "# title\n");
        let raw = m.fetch_one("ENG-1").unwrap();
        assert_eq!(raw.source.system, SourceSystem::Jira);
        assert!(raw.content.contains("title"));
    }

    #[test]
    fn mock_adapter_batch_fetch_loops() {
        let m = MockJiraAdapter::new()
            .with_issue("ENG-1", "one")
            .with_issue("ENG-2", "two");
        let batch = m
            .fetch_batch(&["ENG-1".to_string(), "ENG-2".to_string()])
            .unwrap();
        assert_eq!(batch.len(), 2);
    }

    #[test]
    fn mock_adapter_missing_key_errors() {
        let m = MockJiraAdapter::new();
        let err = m.fetch_one("ENG-999").unwrap_err();
        assert!(matches!(err, ImportError::Adapter(_)));
    }
}

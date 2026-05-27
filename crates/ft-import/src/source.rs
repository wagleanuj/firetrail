//! Provenance descriptors for imported content.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Originating system for an imported record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceSystem {
    /// A markdown file on the local filesystem.
    LocalMarkdown,
    /// A Jira issue fetched through an adapter.
    Jira,
    /// A Confluence page fetched through an adapter.
    Confluence,
    /// A GitHub issue fetched through an adapter.
    GitHubIssue,
}

impl SourceSystem {
    /// Stable lowercase tag used as the label value for
    /// `import:source=<system>`.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::LocalMarkdown => "local_markdown",
            Self::Jira => "jira",
            Self::Confluence => "confluence",
            Self::GitHubIssue => "github_issue",
        }
    }
}

/// Where an imported record came from.
///
/// `url` and `file_path` are optional and independent: a Jira import has a
/// `url` but no path; a markdown import typically has a path but no URL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportSource {
    /// Canonical URL of the source artefact, if applicable.
    pub url: Option<String>,
    /// Local filesystem path of the source file, if applicable.
    pub file_path: Option<PathBuf>,
    /// Originating system.
    pub system: SourceSystem,
}

impl ImportSource {
    /// Helper constructor for a local markdown file.
    #[must_use]
    pub fn local_markdown(path: impl Into<PathBuf>) -> Self {
        Self {
            url: None,
            file_path: Some(path.into()),
            system: SourceSystem::LocalMarkdown,
        }
    }

    /// Human-readable description used in audit messages and labels.
    #[must_use]
    pub fn describe(&self) -> String {
        if let Some(url) = &self.url {
            return url.clone();
        }
        if let Some(path) = &self.file_path {
            return path.display().to_string();
        }
        self.system.tag().to_string()
    }
}

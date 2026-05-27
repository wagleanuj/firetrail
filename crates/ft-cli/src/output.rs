//! Output formatting.
//!
//! Two formats are supported: markdown (TTY default) and JSON (non-TTY
//! default, or `--json` / `--format json`). The [`Formatter`] consumes a
//! [`crate::commands::CommandOutcome`] and emits text to stdout. Errors are
//! rendered through the matching [`crate::error::CliError`] machinery so that
//! every command shares the same envelope.

use std::io::Write;

use is_terminal::IsTerminal;
use serde::Serialize;
use serde_json::json;

use crate::cli::FormatArg;
use crate::commands::CommandOutcome;
use crate::error::CliError;

/// The stable version of the JSON envelope. Bump on breaking changes.
pub const FORMAT_VERSION: u32 = 1;

/// Resolved output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// Human-readable Markdown.
    Markdown,
    /// Machine-readable JSON envelope.
    Json,
}

impl OutputFormat {
    /// Resolve the requested format. Precedence:
    ///
    /// 1. `--json` shortcut.
    /// 2. Explicit `--format <fmt>`.
    /// 3. TTY detection on stdout (markdown if a TTY, json otherwise).
    pub fn resolve(format: Option<FormatArg>, json_flag: bool) -> Self {
        if json_flag {
            return OutputFormat::Json;
        }
        match format {
            Some(FormatArg::Markdown) => OutputFormat::Markdown,
            Some(FormatArg::Json) => OutputFormat::Json,
            None => {
                if std::io::stdout().is_terminal() {
                    OutputFormat::Markdown
                } else {
                    OutputFormat::Json
                }
            }
        }
    }
}

/// Stateless renderer that writes either markdown or JSON to stdout.
#[derive(Debug, Clone)]
pub struct Formatter {
    format: OutputFormat,
    quiet: bool,
}

impl Formatter {
    /// Create a new formatter for the given resolved format.
    #[must_use]
    pub fn new(format: OutputFormat, quiet: bool) -> Self {
        Self { format, quiet }
    }

    /// Currently selected format.
    #[must_use]
    #[allow(dead_code)] // used by work-graph subcommands (firetrail-1xc).
    pub fn format(&self) -> OutputFormat {
        self.format
    }

    /// Whether quiet mode is on.
    #[must_use]
    #[allow(dead_code)] // used by work-graph subcommands (firetrail-1xc).
    pub fn quiet(&self) -> bool {
        self.quiet
    }

    /// Render a successful command outcome.
    pub fn render_ok(&self, command: &str, outcome: &CommandOutcome, elapsed_ms: u64) {
        let stdout = std::io::stdout();
        let mut out = stdout.lock();
        match self.format {
            OutputFormat::Json => {
                let envelope = SuccessEnvelope {
                    format_version: FORMAT_VERSION,
                    command,
                    data: outcome.json_data(),
                    warnings: outcome.warnings(),
                    elapsed_ms,
                };
                if let Ok(s) = serde_json::to_string_pretty(&envelope) {
                    let _ = writeln!(out, "{s}");
                }
            }
            OutputFormat::Markdown => {
                if self.quiet {
                    let line = outcome.quiet_line();
                    if !line.is_empty() {
                        let _ = writeln!(out, "{line}");
                    }
                    return;
                }
                let md = outcome.markdown();
                let _ = writeln!(out, "{md}");
            }
        }
    }

    /// Render an error, sharing the same envelope shape for JSON consumers.
    pub fn render_err(&self, command: &str, err: &CliError, elapsed_ms: u64) {
        match self.format {
            OutputFormat::Json => {
                let envelope = json!({
                    "format_version": FORMAT_VERSION,
                    "command": command,
                    "error": {
                        "code": err.exit_code(),
                        "kind": err.kind(),
                        "message": err.to_string(),
                        "details": err.details(),
                    },
                    "elapsed_ms": elapsed_ms,
                });
                let s = serde_json::to_string_pretty(&envelope)
                    .unwrap_or_else(|_| "{\"error\":\"render failed\"}".to_string());
                let stderr = std::io::stderr();
                let _ = writeln!(stderr.lock(), "{s}");
            }
            OutputFormat::Markdown => {
                let stderr = std::io::stderr();
                let mut out = stderr.lock();
                let _ = writeln!(out, "error: {err}");
                let details = err.details();
                if !details.is_null() && !self.quiet {
                    if let Ok(s) = serde_json::to_string_pretty(&details) {
                        let _ = writeln!(out, "{s}");
                    }
                }
            }
        }
    }
}

#[derive(Serialize)]
struct SuccessEnvelope<'a> {
    format_version: u32,
    command: &'a str,
    data: serde_json::Value,
    warnings: Vec<String>,
    elapsed_ms: u64,
}

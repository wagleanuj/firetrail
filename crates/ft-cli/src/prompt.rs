//! Minimal interactive-prompt helpers for CLI commands that need a yes/no/skip
//! decision per candidate (e.g. `promote-import --interactive`, `memory
//! salvage`). Falls back to a default when stdin is not a TTY.

use std::io::{self, BufRead, IsTerminal, Write};

/// User decision for a single candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptChoice {
    /// Accept (promote / salvage).
    Yes,
    /// Reject (skip / leave alone).
    No,
    /// Stop processing further candidates.
    Quit,
}

/// True when stdin and stdout are both TTYs and the caller can prompt.
#[must_use]
pub fn is_interactive() -> bool {
    io::stdin().is_terminal() && io::stdout().is_terminal()
}

/// Prompt the user with `question` (e.g. `"promote ABC-123? [y/N/q]"`).
///
/// Returns the default when stdin is non-TTY, when the line is empty, or when
/// stdin EOFs. Errors propagate only for `flush` / `read_line` IO failures.
pub fn ask(question: &str, default: PromptChoice) -> io::Result<PromptChoice> {
    if !is_interactive() {
        return Ok(default);
    }
    let stdout = io::stdout();
    {
        let mut lock = stdout.lock();
        write!(lock, "{question} ")?;
        lock.flush()?;
    }
    let stdin = io::stdin();
    let mut line = String::new();
    let read = stdin.lock().read_line(&mut line)?;
    if read == 0 {
        // EOF — fall back to default rather than looping forever.
        return Ok(default);
    }
    Ok(match line.trim().to_ascii_lowercase().as_str() {
        "y" | "yes" => PromptChoice::Yes,
        "n" | "no" => PromptChoice::No,
        "q" | "quit" | "exit" => PromptChoice::Quit,
        _ => default,
    })
}

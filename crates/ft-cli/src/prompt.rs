//! Minimal interactive-prompt helpers for CLI commands that need a yes/no/skip
//! decision per candidate (e.g. `promote-import --interactive`, `memory
//! salvage`, `init`). Falls back to a default when stdin is not a TTY.

use std::io::{self, BufRead, IsTerminal, Write};

/// Prompt for a yes/no answer. Returns `default` on empty input, non-TTY, or
/// EOF. Echoes `[Y/n]` / `[y/N]` based on the default.
pub fn ask_yes_no(question: &str, default: bool) -> io::Result<bool> {
    if !is_interactive() {
        return Ok(default);
    }
    let suffix = if default { "[Y/n]" } else { "[y/N]" };
    let stderr = io::stderr();
    {
        let mut lock = stderr.lock();
        write!(lock, "{question} {suffix} ")?;
        lock.flush()?;
    }
    let mut line = String::new();
    let read = io::stdin().lock().read_line(&mut line)?;
    if read == 0 {
        return Ok(default);
    }
    Ok(match line.trim().to_ascii_lowercase().as_str() {
        "y" | "yes" => true,
        "n" | "no" => false,
        _ => default,
    })
}

/// Prompt for a free-text answer. Returns `default` on empty input, non-TTY,
/// or EOF. The default value (if any) is echoed in brackets.
pub fn ask_text(question: &str, default: Option<&str>) -> io::Result<String> {
    if !is_interactive() {
        return Ok(default.unwrap_or("").to_string());
    }
    let stderr = io::stderr();
    {
        let mut lock = stderr.lock();
        match default {
            Some(d) if !d.is_empty() => write!(lock, "{question} [{d}] ")?,
            _ => write!(lock, "{question} ")?,
        }
        lock.flush()?;
    }
    let mut line = String::new();
    let read = io::stdin().lock().read_line(&mut line)?;
    if read == 0 {
        return Ok(default.unwrap_or("").to_string());
    }
    let trimmed = line.trim();
    if trimmed.is_empty() {
        Ok(default.unwrap_or("").to_string())
    } else {
        Ok(trimmed.to_string())
    }
}

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
///
/// The `FIRETRAIL_FORCE_TTY=1` environment variable forces a `true` return.
/// This is a test-only escape hatch — integration tests pipe scripted input
/// over stdin and need the prompt path to engage even though `cargo test`
/// doesn't allocate a pty. Production callers should never set it.
#[must_use]
pub fn is_interactive() -> bool {
    if std::env::var("FIRETRAIL_FORCE_TTY").as_deref() == Ok("1") {
        return true;
    }
    io::stdin().is_terminal() && io::stdout().is_terminal()
}

/// Prompt the user with `question` (e.g. `"promote ABC-123? [y/N/q]"`).
///
/// Returns the default when stdin is non-TTY, when the line is empty, or when
/// stdin EOFs. Errors propagate only for `flush` / `read_line` IO failures.
///
/// Prompts are written to **stderr** so they don't pollute the JSON stdout
/// channel that callers consume programmatically.
pub fn ask(question: &str, default: PromptChoice) -> io::Result<PromptChoice> {
    if !is_interactive() {
        return Ok(default);
    }
    let stderr = io::stderr();
    {
        let mut lock = stderr.lock();
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

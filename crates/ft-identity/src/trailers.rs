//! Git trailer parsers for identity attribution.
//!
//! Two trailers feed the identity system:
//!
//! - `On-behalf-of: <id>` — a bot or CI runner asserting that it is acting
//!   for the given canonical identity. Trust transitions credit the
//!   on-behalf-of identity, not the bot.
//! - `Co-authored-by: Name <email>` — standard git trailer extending
//!   authorship to a second identity (pair programming).
//!
//! Trailers are recognized in the standard git format: they appear in the
//! final "trailer block" of the commit message, separated from the body by
//! at least one blank line. We parse leniently — any line in the message
//! matching the syntax is accepted, regardless of position — because
//! upstream tools mangle whitespace in ways that block strict implementations.

use ft_core::Identity;

/// Trailer key for "acting on behalf of another identity".
const ON_BEHALF_OF: &str = "on-behalf-of";
/// Trailer key for "co-authorship", per git's well-known convention.
const CO_AUTHORED_BY: &str = "co-authored-by";

/// Return the value of the first `On-behalf-of:` trailer in `commit_message`,
/// if any.
///
/// The lookup is case-insensitive on the trailer key. The returned string is
/// trimmed but otherwise unmodified — callers resolve it to a
/// [`crate::registry::RegisteredIdentity`] via
/// [`crate::registry::IdentityRegistry::resolve_canonical`].
#[must_use]
pub fn parse_on_behalf_of(commit_message: &str) -> Option<String> {
    for line in commit_message.lines() {
        if let Some((key, value)) = split_trailer(line) {
            if key.eq_ignore_ascii_case(ON_BEHALF_OF) {
                return Some(value.to_string());
            }
        }
    }
    None
}

/// Extract every `Co-authored-by:` trailer from `commit_message` as an
/// [`Identity`]. The standard format is `Name <email>`; we use the email.
///
/// Entries that fail [`Identity::new`] validation are silently skipped —
/// commit messages routinely contain malformed trailers from upstream
/// machinery, and surfacing those as errors would block legitimate work.
#[must_use]
pub fn co_authors(commit_message: &str) -> Vec<Identity> {
    let mut out = Vec::new();
    for line in commit_message.lines() {
        let Some((key, value)) = split_trailer(line) else {
            continue;
        };
        if !key.eq_ignore_ascii_case(CO_AUTHORED_BY) {
            continue;
        }
        let email = extract_email(value).unwrap_or(value);
        if let Ok(id) = Identity::new(email) {
            out.push(id);
        }
    }
    out
}

/// Split a line of the form `key: value` into `(key, value)`, trimming
/// surrounding whitespace.
fn split_trailer(line: &str) -> Option<(&str, &str)> {
    let trimmed = line.trim();
    let (k, v) = trimmed.split_once(':')?;
    let k = k.trim();
    let v = v.trim();
    if k.is_empty() || v.is_empty() {
        return None;
    }
    // Keys are tokens; reject obvious non-trailer lines like `1: foo` only
    // if they contain whitespace or special punctuation in the key.
    if k.chars().any(char::is_whitespace) {
        return None;
    }
    Some((k, v))
}

/// Pull the `<email>` portion out of `Name <email>`.
fn extract_email(value: &str) -> Option<&str> {
    let start = value.find('<')?;
    let end = value[start..].find('>')?;
    let email = &value[start + 1..start + end];
    if email.is_empty() { None } else { Some(email) }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_on_behalf_of_basic() {
        let msg = "Fix the thing\n\nOn-behalf-of: alice\n";
        assert_eq!(parse_on_behalf_of(msg).as_deref(), Some("alice"));
    }

    #[test]
    fn parse_on_behalf_of_case_insensitive_key() {
        let msg = "Body\n\nOn-Behalf-Of: bob@example.com\n";
        assert_eq!(parse_on_behalf_of(msg).as_deref(), Some("bob@example.com"));
    }

    #[test]
    fn parse_on_behalf_of_absent_returns_none() {
        assert_eq!(parse_on_behalf_of("just a commit message"), None);
    }

    #[test]
    fn parse_on_behalf_of_picks_first() {
        let msg = "Body\n\nOn-behalf-of: alice\nOn-behalf-of: bob\n";
        assert_eq!(parse_on_behalf_of(msg).as_deref(), Some("alice"));
    }

    #[test]
    fn co_authors_extracts_emails() {
        let msg = "Fix\n\nCo-authored-by: Alice <alice@example.com>\nCo-authored-by: Bob <bob@example.com>\n";
        let got = co_authors(msg);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].as_str(), "alice@example.com");
        assert_eq!(got[1].as_str(), "bob@example.com");
    }

    #[test]
    fn co_authors_skips_malformed() {
        let msg = "Co-authored-by: no email here\nCo-authored-by: Bob <bob@example.com>\n";
        let got = co_authors(msg);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].as_str(), "bob@example.com");
    }

    #[test]
    fn co_authors_empty_when_no_trailers() {
        assert!(co_authors("plain message").is_empty());
    }
}

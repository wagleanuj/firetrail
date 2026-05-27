//! Section-detecting markdown parsers for incident reports, ADRs, and
//! runbooks.
//!
//! The parsers do not interpret inline markdown — they tokenize the document
//! into `(header, body)` pairs by scanning for `#` lines and accumulating the
//! lines that follow. Case-insensitive header matching means a file written
//! with `## ROOT CAUSE` or `## Root cause` lands in the same bucket.

use crate::error::ImportError;
use crate::source::ImportSource;

// ---------------------------------------------------------------------------
// Parsed shapes
// ---------------------------------------------------------------------------

/// Result of parsing a markdown incident report.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedIncident {
    /// Document title (H1, or first non-empty line if no H1 was present).
    pub title: String,
    /// Body of the `## Symptoms` section, if present.
    pub summary: Option<String>,
    /// Body of the `## Root Cause` section, if present.
    pub root_cause: Option<String>,
    /// Body of the `## Resolution` section, if present.
    pub resolution: Option<String>,
    /// Items extracted from the `## Action Items` section (one entry per
    /// bullet or numbered list item).
    pub action_items: Vec<String>,
    /// Body of the `## Lessons Learned` section, if present.
    pub lessons_learned: Option<String>,
    /// The full original markdown body (verbatim).
    pub raw_body: String,
    /// Fraction `(detected_sections / 5.0)` describing how well-structured the
    /// source was. `1.0` means all five expected sections were found.
    pub parse_confidence: f32,
}

/// Result of parsing an ADR markdown file.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedAdr {
    /// ADR number, if the H1 follows the `ADR-NNNN: Title` convention.
    pub number: Option<u32>,
    /// Document title (with the optional `ADR-NNNN:` prefix stripped).
    pub title: String,
    /// Body of the `## Status` section, if present.
    pub status: Option<String>,
    /// Body of the `## Context` section, if present.
    pub context: Option<String>,
    /// Body of the `## Decision` section, if present.
    pub decision: Option<String>,
    /// Body of the `## Consequences` section, if present.
    pub consequences: Option<String>,
    /// Items from the `## Alternatives Considered` section, if present.
    pub alternatives: Vec<String>,
    /// The full original markdown body.
    pub raw_body: String,
    /// `(detected_sections / 4.0)` over Status/Context/Decision/Consequences.
    pub parse_confidence: f32,
}

/// A single parsed runbook step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunbookStep {
    /// Step description (the bullet text or `## Step N` body).
    pub description: String,
}

/// Result of parsing a runbook markdown file.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedRunbook {
    /// Document title (H1).
    pub title: String,
    /// Body of the `## Summary` section, if present.
    pub summary: Option<String>,
    /// Ordered list of steps detected in the document.
    pub steps: Vec<RunbookStep>,
    /// Comma-separated values from `## Applies To`, if present.
    pub applies_to: Vec<String>,
    /// The full original markdown body.
    pub raw_body: String,
    /// `(detected_sections / 3.0)` over Summary/Steps/Applies-To.
    pub parse_confidence: f32,
}

// ---------------------------------------------------------------------------
// Internal: section tokenizer
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Section {
    /// Header level (1 for `#`, 2 for `##`, ...).
    level: u8,
    /// Header text (lowercased, trimmed).
    heading_lower: String,
    /// Header text as written (trimmed of leading hashes and whitespace).
    heading_original: String,
    /// Body lines following the header up to the next header of equal or
    /// shallower level.
    body: String,
}

/// Split `input` into a leading prelude (anything before the first `#`
/// header) and a vector of sections.
fn tokenize(input: &str) -> (String, Vec<Section>) {
    let mut sections: Vec<Section> = Vec::new();
    let mut prelude_lines: Vec<&str> = Vec::new();
    let mut current_header: Option<(u8, String)> = None;
    let mut current_body: Vec<&str> = Vec::new();

    for line in input.lines() {
        if let Some((level, text)) = parse_header(line) {
            // Flush any in-progress section.
            if let Some((lvl, head)) = current_header.take() {
                let body = current_body.join("\n").trim_matches('\n').to_string();
                sections.push(Section {
                    level: lvl,
                    heading_lower: head.to_lowercase(),
                    heading_original: head,
                    body,
                });
                current_body.clear();
            } else {
                // Lines before the first header are the prelude.
                // (Nothing to flush; prelude_lines already captured them.)
            }
            current_header = Some((level, text));
        } else if current_header.is_none() {
            prelude_lines.push(line);
        } else {
            current_body.push(line);
        }
    }

    if let Some((lvl, head)) = current_header.take() {
        let body = current_body.join("\n").trim_matches('\n').to_string();
        sections.push(Section {
            level: lvl,
            heading_lower: head.to_lowercase(),
            heading_original: head,
            body,
        });
    }

    let prelude = prelude_lines.join("\n");
    (prelude, sections)
}

/// Parse a single line as an ATX-style markdown header. Returns
/// `(level, text)` if the line starts with one to six `#` followed by a
/// space; otherwise `None`.
fn parse_header(line: &str) -> Option<(u8, String)> {
    let trimmed = line.trim_start();
    if !trimmed.starts_with('#') {
        return None;
    }
    let mut level: u8 = 0;
    let mut chars = trimmed.chars();
    for c in chars.by_ref() {
        if c == '#' {
            level = level.saturating_add(1);
        } else {
            // The character that terminated the run must be whitespace; if
            // it isn't, this isn't a header (e.g. `#footnote`).
            if !c.is_whitespace() {
                return None;
            }
            break;
        }
        if level > 6 {
            return None;
        }
    }
    if level == 0 {
        return None;
    }
    let rest: String = chars.collect();
    let text = rest.trim().trim_end_matches('#').trim().to_string();
    if text.is_empty() {
        return None;
    }
    Some((level, text))
}

/// Find the H1 in the section list; otherwise fall back to the first
/// non-empty prelude line, otherwise the first section heading.
fn detect_title(prelude: &str, sections: &[Section]) -> Option<String> {
    if let Some(h1) = sections.iter().find(|s| s.level == 1) {
        return Some(h1.heading_original.clone());
    }
    if let Some(first_nonempty) = prelude.lines().map(str::trim).find(|l| !l.is_empty()) {
        return Some(first_nonempty.to_string());
    }
    sections.first().map(|s| s.heading_original.clone())
}

/// Match a section by exact (lowercased) heading among a list of accepted
/// names. Returns the trimmed body of the first match.
fn find_section<'a>(sections: &'a [Section], accept: &[&str]) -> Option<&'a Section> {
    sections
        .iter()
        .find(|s| accept.iter().any(|name| s.heading_lower == *name))
}

/// Extract bullet / numbered list items from a body. Lines that are not list
/// items are ignored; a line whose first non-whitespace character is one of
/// `-`, `*`, `+`, or that looks like `N.` / `N)` is treated as an item.
/// Multi-line items (continuation indented under a bullet) are joined with a
/// single space.
fn extract_list_items(body: &str) -> Vec<String> {
    let mut items: Vec<String> = Vec::new();
    let mut current: Option<String> = None;
    for line in body.lines() {
        if let Some(rest) = strip_list_marker(line) {
            if let Some(prev) = current.take() {
                let trimmed = prev.trim().to_string();
                if !trimmed.is_empty() {
                    items.push(trimmed);
                }
            }
            current = Some(rest.to_string());
        } else if let Some(buf) = current.as_mut() {
            let t = line.trim();
            if t.is_empty() {
                // Blank line ends the current item.
                let trimmed = buf.trim().to_string();
                if !trimmed.is_empty() {
                    items.push(trimmed);
                }
                current = None;
            } else {
                buf.push(' ');
                buf.push_str(t);
            }
        }
    }
    if let Some(prev) = current.take() {
        let trimmed = prev.trim().to_string();
        if !trimmed.is_empty() {
            items.push(trimmed);
        }
    }
    items
}

/// If `line` begins (after leading whitespace) with a list marker, return
/// the remainder of the line after the marker.
fn strip_list_marker(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    if let Some(rest) = trimmed.strip_prefix("- ") {
        return Some(rest);
    }
    if let Some(rest) = trimmed.strip_prefix("* ") {
        return Some(rest);
    }
    if let Some(rest) = trimmed.strip_prefix("+ ") {
        return Some(rest);
    }
    // Numbered: `1. text` or `1) text`.
    let mut iter = trimmed.char_indices();
    let mut last_digit_end = 0;
    let mut saw_digit = false;
    for (i, c) in iter.by_ref() {
        if c.is_ascii_digit() {
            saw_digit = true;
            last_digit_end = i + 1;
        } else {
            if !saw_digit {
                return None;
            }
            if (c == '.' || c == ')')
                && let Some(after_punct) = trimmed.get(last_digit_end + 1..)
                && let Some(stripped) = after_punct.strip_prefix(' ')
            {
                return Some(stripped);
            }
            return None;
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Incident
// ---------------------------------------------------------------------------

/// Parse a markdown incident report.
///
/// Detects the five canonical sections (Symptoms, Root Cause, Resolution,
/// Action Items, Lessons Learned), produces a structured [`ParsedIncident`],
/// and reports a `parse_confidence` proportional to how many of the five were
/// found.
///
/// # Errors
///
/// Returns [`ImportError::Empty`] if `input` is empty or contains only
/// whitespace, and [`ImportError::Parse`] if no title can be derived.
pub fn parse_incident_md(
    input: &str,
    _source: &ImportSource,
) -> Result<ParsedIncident, ImportError> {
    if input.trim().is_empty() {
        return Err(ImportError::Empty("incident markdown".into()));
    }
    let (prelude, sections) = tokenize(input);
    let title = detect_title(&prelude, &sections)
        .ok_or_else(|| ImportError::Parse("incident: no title found".into()))?;

    let summary =
        find_section(&sections, &["symptoms", "symptom", "summary"]).map(|s| s.body.clone());
    let root_cause =
        find_section(&sections, &["root cause", "root-cause", "cause"]).map(|s| s.body.clone());
    let resolution =
        find_section(&sections, &["resolution", "fix", "mitigation"]).map(|s| s.body.clone());
    let action_items_section =
        find_section(&sections, &["action items", "action-items", "actions"]);
    let action_items = action_items_section
        .map(|s| extract_list_items(&s.body))
        .unwrap_or_default();
    let lessons_learned = find_section(
        &sections,
        &["lessons learned", "lessons-learned", "lessons"],
    )
    .map(|s| s.body.clone());

    let mut found = 0_u8;
    if summary.is_some() {
        found += 1;
    }
    if root_cause.is_some() {
        found += 1;
    }
    if resolution.is_some() {
        found += 1;
    }
    if action_items_section.is_some() {
        found += 1;
    }
    if lessons_learned.is_some() {
        found += 1;
    }
    let parse_confidence = f32::from(found) / 5.0_f32;

    Ok(ParsedIncident {
        title,
        summary,
        root_cause,
        resolution,
        action_items,
        lessons_learned,
        raw_body: input.to_string(),
        parse_confidence,
    })
}

// ---------------------------------------------------------------------------
// ADR
// ---------------------------------------------------------------------------

/// Parse an ADR markdown file.
///
/// Detects the title (H1, including the `ADR-NNNN:` prefix convention) plus
/// the canonical Status / Context / Decision / Consequences / Alternatives
/// sections.
///
/// # Errors
///
/// Same conditions as [`parse_incident_md`].
pub fn parse_adr_md(input: &str, _source: &ImportSource) -> Result<ParsedAdr, ImportError> {
    if input.trim().is_empty() {
        return Err(ImportError::Empty("adr markdown".into()));
    }
    let (prelude, sections) = tokenize(input);
    let raw_title = detect_title(&prelude, &sections)
        .ok_or_else(|| ImportError::Parse("adr: no title found".into()))?;
    let (number, title) = split_adr_title(&raw_title);

    let status = find_section(&sections, &["status"]).map(|s| s.body.clone());
    let context = find_section(&sections, &["context"]).map(|s| s.body.clone());
    let decision = find_section(&sections, &["decision"]).map(|s| s.body.clone());
    let consequences = find_section(&sections, &["consequences"]).map(|s| s.body.clone());
    let alternatives_section = find_section(
        &sections,
        &[
            "alternatives considered",
            "alternatives",
            "alternatives-considered",
        ],
    );
    let alternatives = alternatives_section
        .map(|s| {
            let items = extract_list_items(&s.body);
            if items.is_empty() && !s.body.trim().is_empty() {
                // Fall back to a single blob if the section is prose.
                vec![s.body.trim().to_string()]
            } else {
                items
            }
        })
        .unwrap_or_default();

    let mut found = 0_u8;
    if status.is_some() {
        found += 1;
    }
    if context.is_some() {
        found += 1;
    }
    if decision.is_some() {
        found += 1;
    }
    if consequences.is_some() {
        found += 1;
    }
    let parse_confidence = f32::from(found) / 4.0_f32;

    Ok(ParsedAdr {
        number,
        title,
        status,
        context,
        decision,
        consequences,
        alternatives,
        raw_body: input.to_string(),
        parse_confidence,
    })
}

/// Split an ADR title of the form `ADR-NNNN: Title` (case-insensitive) into
/// `(Some(NNNN), "Title")`. Returns `(None, raw)` if the prefix isn't
/// present.
fn split_adr_title(raw: &str) -> (Option<u32>, String) {
    // Look for "adr-" prefix (case-insensitive) followed by digits, then ':'.
    let lower = raw.to_lowercase();
    if let Some(rest) = lower.strip_prefix("adr-") {
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        if !digits.is_empty()
            && let Ok(n) = digits.parse::<u32>()
        {
            // Find the `:` in the original (preserves case) after the digits.
            let prefix_len = "adr-".len() + digits.len();
            if let Some(after_digits) = raw.get(prefix_len..) {
                let rest_trimmed = after_digits.trim_start();
                if let Some(after_colon) = rest_trimmed.strip_prefix(':') {
                    return (Some(n), after_colon.trim().to_string());
                }
            }
        }
    }
    (None, raw.to_string())
}

// ---------------------------------------------------------------------------
// Runbook
// ---------------------------------------------------------------------------

/// Parse a runbook markdown file.
///
/// Detects the title (H1), an optional `## Summary` section, an `## Applies
/// To` section (parsed as comma-separated values), and steps. Steps come
/// from either a `## Steps` section (numbered or bulleted list) or a series
/// of `## Step N` headers.
///
/// # Errors
///
/// Same conditions as [`parse_incident_md`].
pub fn parse_runbook_md(input: &str, _source: &ImportSource) -> Result<ParsedRunbook, ImportError> {
    if input.trim().is_empty() {
        return Err(ImportError::Empty("runbook markdown".into()));
    }
    let (prelude, sections) = tokenize(input);
    let title = detect_title(&prelude, &sections)
        .ok_or_else(|| ImportError::Parse("runbook: no title found".into()))?;

    let summary = find_section(&sections, &["summary", "overview"]).map(|s| s.body.clone());

    let applies_to_section = find_section(
        &sections,
        &["applies to", "applies-to", "appliesto", "scope"],
    );
    let applies_to = applies_to_section
        .map(|s| {
            // Try list-item style first; fall back to comma-split prose.
            let items = extract_list_items(&s.body);
            if items.is_empty() {
                s.body
                    .split([',', '\n'])
                    .map(str::trim)
                    .filter(|x| !x.is_empty())
                    .map(ToString::to_string)
                    .collect()
            } else {
                items
            }
        })
        .unwrap_or_default();

    // Steps: prefer a single `## Steps` section, otherwise gather `## Step N`
    // headers.
    let mut steps: Vec<RunbookStep> = Vec::new();
    if let Some(steps_section) = find_section(&sections, &["steps", "procedure"]) {
        let items = extract_list_items(&steps_section.body);
        if items.is_empty() && !steps_section.body.trim().is_empty() {
            // Paragraph-style steps section: keep as one step.
            steps.push(RunbookStep {
                description: steps_section.body.trim().to_string(),
            });
        } else {
            for item in items {
                steps.push(RunbookStep { description: item });
            }
        }
    } else {
        for sec in &sections {
            if sec.heading_lower.starts_with("step ") || sec.heading_lower.starts_with("step\t") {
                steps.push(RunbookStep {
                    description: sec.body.trim().to_string(),
                });
            }
        }
    }

    let mut found = 0_u8;
    if summary.is_some() {
        found += 1;
    }
    if !steps.is_empty() {
        found += 1;
    }
    if applies_to_section.is_some() {
        found += 1;
    }
    let parse_confidence = f32::from(found) / 3.0_f32;

    Ok(ParsedRunbook {
        title,
        summary,
        steps,
        applies_to,
        raw_body: input.to_string(),
        parse_confidence,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::ImportSource;

    fn src() -> ImportSource {
        ImportSource::local_markdown("test.md")
    }

    const INCIDENT_FULL: &str = "# Redis pool exhaustion 2023-08-21

## Symptoms

Checkout requests timing out across the fleet.

## Root Cause

A bad config push reduced the Redis pool size from 200 to 20.

## Resolution

Reverted the config push; pool size restored.

## Action Items

- Add an alert on pool size config changes
- Add a guard in the config-push tool to block <50 pool

## Lessons Learned

Config changes touching pool sizes need a second reviewer.
";

    #[test]
    fn parse_incident_full_returns_confidence_one() {
        let parsed = parse_incident_md(INCIDENT_FULL, &src()).unwrap();
        assert_eq!(parsed.title, "Redis pool exhaustion 2023-08-21");
        assert!(parsed.summary.as_ref().unwrap().contains("timing out"));
        assert!(parsed.root_cause.as_ref().unwrap().contains("pool size"));
        assert!(parsed.resolution.as_ref().unwrap().contains("Reverted"));
        assert_eq!(parsed.action_items.len(), 2);
        assert!(parsed.lessons_learned.is_some());
        assert!((parsed.parse_confidence - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn parse_incident_missing_two_sections_confidence_zero_point_six() {
        let input = "# An incident

## Symptoms

It broke.

## Root Cause

It was bad.

## Resolution

We fixed it.
";
        let parsed = parse_incident_md(input, &src()).unwrap();
        assert!(parsed.summary.is_some());
        assert!(parsed.root_cause.is_some());
        assert!(parsed.resolution.is_some());
        assert!(parsed.action_items.is_empty());
        assert!(parsed.lessons_learned.is_none());
        assert!((parsed.parse_confidence - 0.6).abs() < 1e-5);
    }

    #[test]
    fn parse_incident_empty_input_fails() {
        let err = parse_incident_md("   \n\n", &src()).unwrap_err();
        assert!(matches!(err, ImportError::Empty(_)));
    }

    #[test]
    fn parse_incident_case_insensitive_headers() {
        let input = "# x\n\n## ROOT CAUSE\n\nfoo\n\n## resolution\n\nbar\n";
        let parsed = parse_incident_md(input, &src()).unwrap();
        assert!(parsed.root_cause.is_some());
        assert!(parsed.resolution.is_some());
    }

    const ADR_FULL: &str = "# ADR-0042: Use Quic over TCP

## Status

Accepted — 2026-04-01

## Context

TCP head-of-line blocking hurts our streaming latency.

## Decision

Adopt QUIC for the streaming endpoint.

## Consequences

We will need to maintain a QUIC library and update load balancers.

## Alternatives Considered

- Stay on TCP and accept the latency
- Use HTTP/3 with HTTP semantics
";

    #[test]
    fn parse_adr_full() {
        let parsed = parse_adr_md(ADR_FULL, &src()).unwrap();
        assert_eq!(parsed.number, Some(42));
        assert_eq!(parsed.title, "Use Quic over TCP");
        assert!(parsed.status.as_ref().unwrap().contains("Accepted"));
        assert!(parsed.context.is_some());
        assert!(parsed.decision.is_some());
        assert!(parsed.consequences.is_some());
        assert_eq!(parsed.alternatives.len(), 2);
        assert!((parsed.parse_confidence - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn parse_adr_without_number_prefix() {
        let input = "# Some Decision\n\n## Decision\n\nPick A.\n";
        let parsed = parse_adr_md(input, &src()).unwrap();
        assert_eq!(parsed.number, None);
        assert_eq!(parsed.title, "Some Decision");
    }

    const RUNBOOK_FULL: &str = "# Restart the Redis pool

## Summary

Use this when the pool is wedged.

## Applies To

- redis-prod
- redis-stage

## Steps

1. Notify #oncall
2. Drain traffic
3. Restart pool nodes one at a time
4. Verify
";

    #[test]
    fn parse_runbook_full() {
        let parsed = parse_runbook_md(RUNBOOK_FULL, &src()).unwrap();
        assert_eq!(parsed.title, "Restart the Redis pool");
        assert!(parsed.summary.is_some());
        assert_eq!(parsed.applies_to.len(), 2);
        assert_eq!(parsed.steps.len(), 4);
        assert!((parsed.parse_confidence - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn extract_list_items_handles_dash_and_numbered() {
        let body = "- one\n- two\n1. three\n2. four";
        let items = extract_list_items(body);
        assert_eq!(items, vec!["one", "two", "three", "four"]);
    }

    #[test]
    fn parse_header_recognizes_atx() {
        assert_eq!(parse_header("## Title"), Some((2, "Title".to_string())));
        assert_eq!(parse_header("# Title #"), Some((1, "Title".to_string())));
        assert_eq!(parse_header("not # a header"), None);
        assert_eq!(parse_header("#nospace"), None);
    }
}

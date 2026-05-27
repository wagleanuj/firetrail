# ADR-0012: The Claude Code skill is agent documentation, not a separate product tier

## Status

Accepted — 2026-05-26

## Context

Earlier in design we proposed two distribution tiers:

- **Tier 1: Skill only.** A Claude Code skill that teaches Claude markdown conventions for tasks and incidents. No CLI required.
- **Tier 2: Skill + CLI.** The skill detects the CLI and uses it; otherwise falls back to markdown mode.

The motivation was a low-friction adoption path for solo developers and pilots.

Two problems surfaced:

1. **Firetrail is built for teams, not solo developers.** Solo-developer adoption is out of scope.
2. **Tier 1 creates a silent partition of memory.** Tier-1 markdown findings never enter the vector index. Tier-2 agents searching the same repo miss them entirely. Two write paths produce two parallel knowledge stores that drift apart.

The skill itself still has value — teaching Claude how to use Firetrail well is a real artifact. But it should not be an alternative implementation of Firetrail.

## Decision

Firetrail has one product: the CLI. The skill is documentation for AI agents on how to use the CLI well.

### What the skill is

The skill is a markdown file (or set of files) installed at `.claude/skills/firetrail/SKILL.md` by `firetrail init`. It contains instructions for Claude Code on:

- When to invoke `firetrail prime` to load context.
- How to interpret the prime output.
- When to capture findings, decisions, runbooks.
- How to enforce review and evidence requirements.
- Pre-close checklists for tasks.
- Memory hygiene rules.

The skill is not a fallback. It is not a parallel write path. It assumes the CLI is installed.

### What the skill is not

- The skill does not implement record creation in markdown when the CLI is unavailable.
- The skill does not provide a "no-install" tier.
- The skill is not the source of truth for any behavior — the CLI is.

### Adoption posture

A team installing Firetrail runs `firetrail init`. The init flow installs the CLI (the binary is the install) and writes the skill into `.claude/skills/firetrail/`. Team members who use Claude Code automatically get agent-driven workflows; team members who do not use AI agents still get the full CLI surface.

There is no "use the skill alone" path.

## Consequences

Positive:

- One product, one write path, one index. No silent knowledge partition.
- The CLI binary is the unit of installation. Distribution is simple.
- The skill remains valuable as agent documentation — the use case that motivated it survives.
- Agent behavior is centralized in one updatable file rather than spread across implementation tiers.
- Eliminates an entire class of failure scenarios (markdown-only findings invisible to vector search).

Negative:

- A team cannot evaluate Firetrail's workflow without installing the CLI. Acceptable — the CLI is a single binary download.
- Engineers who refuse to install any tool cannot use Firetrail. Acceptable — Firetrail is a team tool, and a team adopts it together.

## Consequences for design

- The skill's content is part of the Firetrail repository and is versioned with the CLI.
- `firetrail init` writes the skill alongside other config.
- `firetrail doctor` checks that the skill is present and up to date.
- Skill updates ship with CLI releases.
- Other AI agents (Cursor, generic LLMs) can read the skill or an equivalent prompt — the skill format is markdown, broadly readable.

## Alternatives considered

**Keep both tiers.** Disqualified by the silent-partition failure mode and by the team-only audience.

**Skill writes JSON via a template, indexed in CI.** Possible but heavy. The team installs a CI workflow that watches `docs/`, parses markdown, and writes JSON. Adds complexity for a use case (no-CLI team adoption) we are not targeting.

**No skill at all — just CLI.** Loses the agent-instruction artifact. The skill is a small file with real value for Claude Code users.

## References

- ADR-0005: No LLM calls from the tool itself
- ADR-0006: Storage modes

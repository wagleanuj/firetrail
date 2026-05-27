# ADR-0005: Firetrail does not call LLMs at runtime

## Status

Accepted — 2026-05-26

## Context

The original spec described several AI-assisted features: `criteria generate`, conversational `capture`, incident-to-memory extraction, memory quality scoring, memory linting, PR summary generation. The implicit assumption was that Firetrail would call an LLM API to power these.

Three observations changed the calculus:

1. Anthropic provides no embedding endpoint, and Claude Code SDK does not offer a hosted "reason about this structured data" API. Embedding requires a separate provider regardless.
2. Firetrail is run inside a Claude Code session. The host agent is already reasoning over the user's context. Having the tool also call an LLM duplicates work and doubles cost.
3. Most "AI" features in the spec are actually rule-based, template-based, or pattern-matching tasks that an LLM is not required for.

A clearer separation emerged: Firetrail produces structured context and enforces structural guardrails; the host agent (Claude Code, Cursor, or a human) does the reasoning.

## Decision

Firetrail does not call any LLM at runtime. The tool itself requires no API key for any reasoning task.

When Firetrail runs inside an AI coding agent, the agent is the reasoning layer:

- The agent reads `firetrail prime` output as structured context.
- The agent generates acceptance criteria, identifies similar incidents, drafts findings, summarizes memory.
- The agent executes `firetrail criteria add`, `firetrail capture`, `firetrail finding create` to persist results.

When Firetrail runs without an AI agent — typically in CI or via a human at the CLI — every feature still works using templates, heuristics, and rule-based logic.

Embeddings, which require a small ML model at runtime, are the one exception. They are produced by a local ONNX model (`bge-small-en-v1.5` by default) and are not LLM calls. Embeddings are addressed by ADR-0007.

## Consequences

Positive:

- Zero API cost from the tool. Token cost stays in the host session where the user is already paying for it.
- No API keys to manage in CI, no provider lock-in, no failure mode when the user is offline or behind a firewall.
- Same tool behavior whether the user is human-driven or agent-driven. No fork in the codebase for "AI mode" vs "manual mode."
- Eliminates an entire class of failure modes — token costs spiraling, LLM hallucinating into records, prompt injection through imported markdown.
- Aligns with NFR-016: Firetrail does not send repository content to external services without explicit configuration.

Negative:

- Features framed as "AI-assisted" in the original spec become "agent-assisted" or "template-driven." Wording in the requirements doc needs updating. The capability is unchanged when an agent is present.
- The capture flow, when run without an agent, is a structured prompt sequence rather than a conversational interview. Acceptable — engineers can type, and the alternative is a hard dependency on an LLM that adds cost and offline-fragility.

## Consequences for design

- `firetrail prime` becomes the central interface between Firetrail and the agent. Its job is to output excellent structured context within a configurable token budget, deterministically.
- `firetrail capture`, `firetrail finding create`, `firetrail criteria add` accept either interactive input or fully-specified flags. Either path is first-class.
- `firetrail check pr`, `firetrail lint memory`, `firetrail memory duplicates` are entirely rule-based and run in CI without any external service.
- The Claude Code skill ships as documentation that teaches the agent how to use the CLI, not as an alternative to the CLI.

## Alternatives considered

**Hard dependency on Claude SDK.** Disqualified by cost, key management, and the duplication of work the host session already does.

**Pluggable LLM provider (OpenAI / Anthropic / local Ollama).** Adds configuration surface and a fallback story when the provider is unavailable. The host-agent path is cleaner.

**LLM-driven features as opt-in extensions.** Possible later, but unnecessary at v1. If a team really wants Firetrail to call an LLM for a specific feature, it can be added as an explicit subcommand (e.g., `firetrail summarize --using openai`) without affecting the core data plane.

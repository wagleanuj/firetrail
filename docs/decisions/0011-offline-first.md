# ADR-0011: Offline-first contract

## Status

Accepted — 2026-05-26

## Context

Engineers use Firetrail on laptops in moving trains, in conference Wi-Fi blackouts, and at 3am when only the local environment is reliable. CI runners sometimes run in network-isolated environments. Incident response cannot wait for an external dependency to be reachable.

A tool that fails when its embedding provider is unreachable, or that requires a remote MCP server for any local-only operation, is not usable in the moments it matters most.

We also want to avoid silently shipping repository content to external services without explicit team consent (NFR-016 in the original requirements).

## Decision

Firetrail's core commands work without network access. External-system commands are clearly partitioned and fail with helpful messages when offline.

### Always-offline commands

```
init, doctor, daemon (start/stop/status)
task/epic/subtask/bug create, update, close
incident create, finding create, runbook create, decision create, gotcha create
memory create, capture, promote-to-main, deprecate, archive, supersede, merge, redact
claim, unclaim, ready, board, graph, link, dep add/remove
criteria add, list, check, uncheck, evidence
search, similar, prime, history, verify
diff, check pr, lint memory, memory duplicates, memory stale
import (from local files: markdown, ADRs, runbooks)
index rebuild, index refresh
export markdown
identity (registry operations against local config)
```

These commands depend only on local files, the local SQLite index, the local embedding cache, and the local ONNX model. They never reach for the network.

### Network-dependent commands

```
github sync, github link, github create-issue
jira sync, jira link, jira create, jira import
confluence import, confluence import-page, confluence publish, confluence sync-runbook
import confluence (when pulling, not when re-processing local cache)
embeddings refresh (when configured to use a remote provider)
```

These commands explicitly require network. When offline, they fail with:

```
error: `firetrail github sync` requires network access to api.github.com
       Run `firetrail doctor --network` to see all network-dependent operations.
       Local equivalents (no network):
         - `firetrail link TASK-xxx --github-url <url>` records the link without syncing.
```

### External mode is offline-friendly

External mode (ADR-0006) needs the data repo. Once cloned, it is just a local Git repo. Pulls and pushes need network; reads, writes, search, and merge driver runs do not. A developer can work offline for days against the local clone and sync when connectivity returns.

### Embedding model

The default local ONNX model (ADR-0007) ships or downloads on first run and is never refetched at command time. After initial setup, embedding never requires network.

### Skill-mode operation

The Claude Code skill teaches the agent to use the CLI. The skill itself is markdown — no network. The agent's reasoning happens in the host session, which may or may not be online depending on the agent's own setup, but Firetrail does not assume the agent is online.

## Consequences

Positive:

- 3am incident response works on a degraded network.
- CI runs in network-isolated environments without special configuration.
- Privacy posture is clear by default — local-only is the path of least resistance, sending data anywhere is an explicit opt-in.
- Network failures degrade gracefully — they affect integrations, not the core workflow.

Negative:

- Some convenience features (PR auto-summary posting to GitHub) are not available offline. Acceptable.
- Bulk imports from Confluence/Jira require network during the import; the imported records then live locally and the network is no longer needed. Acceptable.
- A first-run embedding model download requires network. Mitigated by `firetrail init` running the download as an explicit step with a clear progress indicator, and by the option to bundle the model in the binary distribution.

## Consequences for design

- Every command's help text declares its network requirement: `network: none | required | optional`.
- `firetrail doctor --network` lists every network-touching feature and its current reachability.
- CI integration documentation specifies that `firetrail check pr` is always offline-capable.
- Integrations (`github`, `jira`, `confluence`) live in their own crates and can be omitted from a minimal build for security-sensitive environments.

## Alternatives considered

**Online-first with offline fallback.** Inverts the default. Disqualified — it teaches users to expect network and breaks during the exact moments network is unavailable.

**Hard offline-only — no integrations.** Loses real value (linking to GitHub, importing from Confluence). Rejected.

## References

- ADR-0005: No LLM calls from the tool itself
- ADR-0007: Local embeddings daemon

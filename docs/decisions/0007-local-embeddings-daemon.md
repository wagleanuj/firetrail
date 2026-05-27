# ADR-0007: Local ONNX embeddings, single daemon, content-hash cache

## Status

Accepted — 2026-05-26

## Context

Firetrail's semantic search depends on vector embeddings of record bodies, acceptance criteria, and incident sections. Three independent concerns shape this:

1. **Offline-first.** The tool must function without network access (ADR-0011). Embedding generation cannot be a hard dependency on a hosted provider.
2. **Concurrent processes.** Engineers run multiple worktrees and multiple Claude Code sessions on one machine. Several CLI processes can call `firetrail capture` or `firetrail finding create` concurrently. Naive inline embedding from each process leads to SQLite write contention, partial writes, and occasional cache corruption.
3. **Cost of re-embedding.** Embedding 50k records is a multi-hour CPU task. Switching worktrees, pulling a colleague's branch, or cloning a repo for the first time should not trigger that cost repeatedly.

## Decision

### Local ONNX model by default

The default embedding model is `bge-small-en-v1.5` (384-dimensional), shipped or fetched on first run via the ONNX Runtime (`ort` crate). Quantized model size is ~33 MB. Inference runs on CPU on any developer laptop; runs on Apple Silicon via CoreML, on Windows/Linux via the standard ONNX backend.

The default produces embeddings without network access, without an API key, without per-token cost.

### Pluggable embedder

The embedder is configurable. Three implementations are supported:

- `local` (default): ONNX + `bge-small-en-v1.5` or any compatible model.
- `remote`: HTTP endpoint, used for team-hosted embedding services or hosted providers (Voyage, OpenAI, Cohere) when a team explicitly opts in.
- `lexical`: BM25 keyword fallback. Used when local inference is too expensive on a particular machine and no remote service is configured. Semantic search degrades visibly — search results carry a `mode: lexical` indicator.

Selection in `.firetrail/config.yml`:

```yaml
embeddings:
  provider: local
  model: bge-small-en-v1.5
  fallback: lexical
```

### Single embedding daemon per repo-hash

Embedding requests from CLI processes are queued to a single long-running daemon process per `(machine, repo)` pair. The daemon:

- Listens on a Unix domain socket at `~/.cache/firetrail/<repo-hash>/embedd.sock`.
- Holds a file lock at `~/.cache/firetrail/<repo-hash>/embedd.lock` to prevent multiple daemons.
- Serializes writes to the embedding SQLite store using WAL mode and `PRAGMA busy_timeout`.
- Lives across CLI invocations and idles efficiently when no requests are pending.

CLI calls enqueue requests and return immediately. The read index is eventually consistent with the embedding cache.

### Content-hash keyed cache

Embeddings are stored against the content hash of the embedded text, not against record IDs. Two consequences:

- Switching worktrees does not invalidate the cache. Same body text produces the same hash produces the same cached embedding.
- Imports that re-process unchanged source files do not re-embed.
- The cache is shared across worktrees on the same machine for the same repo.

Each cache row stores `(content_hash, model_id, model_version, vector, vector_checksum)`.

### Integrity verification

- Each row's `vector_checksum` is verified on read.
- `firetrail doctor` samples N rows and re-embeds them to detect silent corruption.
- The daemon refuses to mix vectors from different `model_id` or `model_version`. Model upgrade is an explicit migration, not a side-effect.

### Model upgrades

Switching the embedding model (for example, from `bge-small-en-v1.5` to `bge-base-en-v1.5`) is a repo-level migration triggered by `firetrail migrate embeddings --to <model>`. Re-embedding the full corpus runs once; the resulting cache is publishable as a pre-built artifact (Git LFS blob, S3 object, GitHub release asset) so other developers and CI runners pull instead of recompute.

## Consequences

Positive:

- Offline by default. No API key required at adoption.
- Concurrent CLI processes do not contend for SQLite writes. The daemon serializes the embedding store cleanly.
- Worktree switches and re-clones do not re-embed unchanged records.
- Model upgrades are opt-in and amortized via shared artifact distribution.
- Lexical fallback ensures the tool stays useful on machines where ONNX inference is too slow.
- Silent embedding-cache corruption is detectable rather than served as confident wrong answers.

Negative:

- The daemon is a process the system must manage. Mitigated by single-binary self-spawning, automatic shutdown after idle, and explicit `firetrail daemon status` / `daemon stop` commands.
- First-run cold start downloads the ONNX model (~33 MB) unless bundled. Mitigated by `firetrail init` running the download as an explicit step with a progress bar.
- Quantized small model produces good but not state-of-the-art retrieval quality. Acceptable for the workload — Firetrail searches a curated, scoped corpus, not the open web. Teams that want higher quality can switch to `bge-base-en-v1.5` or a remote provider.

## Alternatives considered

**Inline embedding from each CLI process.** Causes SQLite contention and occasional partial writes when multiple processes run concurrently. Rejected.

**Remote embedding as the default (Voyage / OpenAI).** Violates offline-first. Adds API key management at adoption. Per-record cost is trivial but the operational friction is real.

**Bundle a larger model by default.** Distribution size becomes a barrier. Quality difference is marginal for this corpus. Rejected.

**No embedding cache; recompute every search.** Performance disaster. Rejected.

**Embedding cache keyed by record ID, not content hash.** Triggers unnecessary re-embedding on every minor edit. Rejected.

## References

- ADR-0002: JSON-in-Git storage
- ADR-0011: Offline-first contract

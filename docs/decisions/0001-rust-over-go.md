# ADR-0001: Rust as the implementation language

## Status

Accepted — 2026-05-26

## Context

Firetrail is a CLI tool that manages structured records (tasks, incidents, findings, runbooks, decisions, memory) with non-trivial lifecycles: trust state transitions, multi-scope routing, identity coalescing, Merkle history chains, claim management, and import quarantines. It runs on engineer laptops and CI runners, must distribute as a single static binary, and is expected to be maintained for years as memory corpora accumulate.

The original spec proposed TypeScript. Earlier in design discussion we considered Go on the strength of Dolt (a Go-native versioned SQL database) as the storage layer. With Dolt dropped in favour of JSON-files-in-Git (ADR-0002), the storage-driven case for Go disappears and the choice reopens.

The team building Firetrail is one human plus AI subagents.

## Decision

Build Firetrail in Rust, 2024 edition.

## Consequences

Positive:

- Compile-time correctness on the record model. Trust state machines, scope routing, and history-chain transitions become exhaustive `match` and `enum` checks rather than runtime bugs. This is exactly the workload Rust's type system rewards.
- Excellent tooling fit: `clap` for CLI, `rusqlite` + `sqlite-vec` for index and vectors, `ort` for ONNX embeddings, `serde_json` for record marshalling, `gix` for git reads, `tokio` for the embedding daemon.
- Single static binary distribution, identical to Go.
- Memory safety for the long-running embedding daemon serving multiple concurrent CLI processes.
- AI-assisted development pairs unusually well with Rust. The borrow checker and exhaustive matching catch the class of mistakes AI agents most often introduce (lifetime errors, unhandled enum variants, mismatched types). Compile errors are a tighter feedback loop than runtime failures in CI.

Negative:

- Roughly 1.5× slower development velocity than Go for equivalent surface area when written by humans. Largely offset by AI assistance — an agent does not get frustrated waiting for the compiler.
- Smaller contributor pool if Firetrail becomes open source, though the Rust developer-tools community (ruff, uv, biome, oxc) is active and growing.
- MCP integration relies on community SDKs rather than an official one. Acceptable for our scope.
- Beads, the closest prior art for work-graph algorithms, is written in Go. Patterns remain readable but cannot be directly copy-pasted.

## Alternatives considered

**Go.** Was the leading choice while Dolt was in scope. Without native Dolt embedding the main advantage disappears. The remaining velocity advantage is partially offset by AI-assisted authoring. Beads precedent is a real but secondary factor.

**TypeScript.** Original spec direction. Rejected: `npm install` friction at team adoption, weaker static typing for a schema-heavy domain, and a worse single-binary distribution story.

**Python.** Strong ML ecosystem but hostile distribution for CLI tools used across an engineering team (pyenv, pip, virtualenv). Rejected.

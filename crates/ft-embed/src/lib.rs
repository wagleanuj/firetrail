//! # ft-embed
//!
//! Local ONNX embedding daemon plus per-record embedding cache. Serves
//! multiple concurrent CLI processes over a local socket.
//!
//! ## Relevant ADRs
//!
//! - ADR-0005 — No LLM in the tool
//! - ADR-0007 — Local embeddings daemon

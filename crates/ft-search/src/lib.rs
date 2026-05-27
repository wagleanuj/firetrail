//! # ft-search
//!
//! Hybrid search over Firetrail records: vector similarity (via `ft-embed`
//! and `sqlite-vec`), lexical matching, and a unified ranking layer.
//!
//! ## Relevant ADRs
//!
//! - ADR-0007 — Local embeddings daemon

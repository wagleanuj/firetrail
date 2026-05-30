//! On-write embedding dispatch for synthetic search docs (firetrail-8z0m.5).
//!
//! firetrail-8z0m.3 indexes scopes, identities, and per-entry audit history as
//! synthetic [`ft_search::IndexDoc`]s at `index rebuild`. firetrail-8z0m.6 also
//! embeds them (and records) at rebuild *when a daemon is already running*. But
//! between rebuilds, editing a scope / identity, or saving a record that
//! appends audit history, would leave the affected synthetic doc with a stale
//! (or missing) vector until the next rebuild.
//!
//! This module wires the same best-effort, **daemon-status-gated** embedding
//! dispatch that records use on write ([`crate::tickets`] /
//! [`crate::memory`]'s `try_dispatch_index_record`) into those synthetic-doc
//! write paths, so semantic search reflects edits immediately.
//!
//! ## Gating policy (mirrors `try_dispatch_index_record` exactly)
//!
//! - Resolve the daemon socket; on failure, give up silently (warn-log only).
//! - Dispatch **only** when [`ft_embed::daemon::status`] is already `Running`.
//!   We never spawn a daemon here — on-write dispatch is opportunistic, and a
//!   freshly-spawned daemon would race the caller's own `SQLite` connection.
//! - Every failure is non-fatal: it is logged and swallowed so a search-layer
//!   hiccup can never block a registry save or record write.

use ft_search::IndexDoc;
use ft_workspace::Workspace;

/// Dispatch `IndexRecord` requests for a batch of synthetic docs, best-effort.
///
/// Shared core for the per-domain helpers below, and the public entry point for
/// callers that already hold the [`IndexDoc`]s (e.g. a record-save path that
/// upserts the audit docs lexically and wants to embed the same batch). Resolves
/// the socket once and applies the `status == Running` gate before sending.
pub fn dispatch_docs(ws: &Workspace, op: &str, docs: &[IndexDoc]) {
    if docs.is_empty() {
        return;
    }
    let socket = match ws.daemon_socket_path() {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(error = %e, op = op, "resolve daemon socket path for synthetic embed");
            return;
        }
    };
    // Never spawn: only dispatch when a daemon is already up. Same gate as
    // records' on-write `try_dispatch_index_record`.
    if ft_embed::daemon::status(&socket) != ft_embed::DaemonStatus::Running {
        return;
    }
    for doc in docs {
        let id = doc.id.as_storage_str();
        if let Err(e) = ft_embed::daemon::send_index_record(&socket, &id, &doc.embed_text()) {
            tracing::warn!(error = %e, op = op, id = %id, "synthetic embed-on-write dispatch failed");
        }
    }
}

/// Re-embed the `scope:<id>` synthetic doc for an edited scope.
///
/// Wire this into any scope edit/save path. (As of firetrail-8z0m.5 the scope
/// registry is read-only — `ScopeRegistry` has no `save` — so this currently
/// has no production call site; it exists so the seam is ready the moment a
/// scope-write op lands.)
pub fn dispatch_scope(ws: &Workspace, op: &str, scope: &ft_scope::Scope) {
    let doc = ft_search::scope_doc(scope, chrono::Utc::now());
    dispatch_docs(ws, op, std::slice::from_ref(&doc));
}

/// Re-embed the `identity:<id>` synthetic doc for an updated identity.
///
/// Wire this into every identity write path (register / update / offboard).
pub fn dispatch_identity(ws: &Workspace, op: &str, identity: &ft_identity::RegisteredIdentity) {
    let doc = ft_search::identity_doc(identity, chrono::Utc::now());
    dispatch_docs(ws, op, std::slice::from_ref(&doc));
}

/// Build the `audit:<id>#h<n>` synthetic docs for a record, with the same trust
/// resolution `index rebuild` uses. Returns an empty vec when the record has no
/// history. Exposed so a record-save path can upsert these lexically and embed
/// the same batch without rebuilding them.
#[must_use]
pub fn audit_docs_for(record: &ft_core::Record) -> Vec<IndexDoc> {
    let trust = audit_record_trust(record);
    ft_search::audit_docs(record, trust)
}

/// Re-embed the `audit:<id>#h<n>` synthetic docs for a record whose save just
/// appended history. Mirrors the trust resolution `index rebuild` uses for
/// audit docs.
pub fn dispatch_audit(ws: &Workspace, op: &str, record: &ft_core::Record) {
    let docs = audit_docs_for(record);
    dispatch_docs(ws, op, &docs);
}

/// Trust an audit doc inherits — mirrors the engine's record-trust rule and the
/// identical helper in `ft_cli::commands::index_cmd` (memory bodies carry
/// trust; work kinds default to reviewed).
fn audit_record_trust(rec: &ft_core::Record) -> ft_core::TrustState {
    use ft_core::{RecordBody, TrustState};
    match &rec.body {
        RecordBody::Incident(b) => b.trust,
        RecordBody::Finding(b) => b.trust,
        RecordBody::Runbook(b) => b.trust,
        RecordBody::Decision(b) => b.trust,
        RecordBody::Gotcha(b) => b.trust,
        RecordBody::Memory(b) => b.trust,
        RecordBody::Doc(b) => b.trust,
        RecordBody::Epic(_) | RecordBody::Task(_) | RecordBody::Subtask(_) | RecordBody::Bug(_) => {
            TrustState::Reviewed
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every entry point must be non-fatal (and must never spawn a daemon) when
    /// no daemon is running — the common hermetic-CI case. We assert they reach
    /// the dispatch code path and return without panicking or starting anything
    /// (no socket exists under the temp workspace → status != Running → no-op).
    #[test]
    fn dispatch_is_non_fatal_without_daemon() {
        let tmp = tempfile::tempdir().unwrap();
        let ws = Workspace::open_uninitialised(tmp.path()).unwrap();

        let scope = ft_scope::Scope {
            id: "apps/checkout".into(),
            name: "Checkout".into(),
            applies_to_patterns: vec!["apps/checkout/**".into()],
            applies_to: Vec::new(),
            aliases: Vec::new(),
            codeowners: None,
        };
        dispatch_scope(&ws, "test scope", &scope);

        let identity = ft_identity::RegisteredIdentity {
            id: "alice".into(),
            name: "Alice".into(),
            kind: ft_identity::IdentityKind::Human,
            emails: vec!["alice@example.com".into()],
            machines: Vec::new(),
            capabilities: ft_identity::PartialCapabilityMatrix::default(),
            status: ft_identity::IdentityStatus::Active,
        };
        dispatch_identity(&ws, "test identity", &identity);
    }
}

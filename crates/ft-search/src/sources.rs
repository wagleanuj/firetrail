//! Pure builders that lower scopes, identities, and audit entries into
//! [`IndexDoc`]s for indexing. Kept in ft-search so the engine owns the
//! title/body shape; the reindex command supplies the domain objects.

use chrono::{DateTime, Utc};
use ft_core::{Record, TrustState};

use crate::engine::IndexDoc;
use crate::kind::{DocId, IndexKind};

/// Lower a scope into a searchable document. CODEOWNERS owner logins (the
/// ownership "rules" nested in a scope) are folded into the body.
#[must_use]
pub fn scope_doc(scope: &ft_scope::Scope, updated_at: DateTime<Utc>) -> IndexDoc {
    let mut body_parts: Vec<String> = Vec::new();
    body_parts.push(scope.id.clone());
    if !scope.aliases.is_empty() {
        body_parts.push(scope.aliases.join(" "));
    }
    body_parts.extend(scope.applies_to_patterns.iter().cloned());
    if let Some(entries) = &scope.codeowners {
        for entry in entries {
            body_parts.push(entry.owners.join(" "));
        }
    }
    IndexDoc {
        id: DocId::Synthetic {
            kind: IndexKind::Scope,
            key: scope.id.clone(),
        },
        kind: IndexKind::Scope,
        title: scope.name.clone(),
        body: body_parts
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        trust: TrustState::Verified,
        owning_scope: Some(scope.id.clone()),
        updated_at,
    }
}

/// Lower a registered identity into a searchable document.
#[must_use]
pub fn identity_doc(
    ident: &ft_identity::RegisteredIdentity,
    updated_at: DateTime<Utc>,
) -> IndexDoc {
    let mut body_parts: Vec<String> = Vec::new();
    body_parts.push(ident.id.clone());
    body_parts.extend(ident.emails.iter().cloned());
    body_parts.extend(ident.machines.iter().cloned());
    let caps = ident.effective_capabilities();
    for (name, enabled) in &caps.extra {
        if *enabled {
            body_parts.push(name.clone());
        }
    }
    IndexDoc {
        id: DocId::Synthetic {
            kind: IndexKind::Identity,
            key: ident.id.clone(),
        },
        kind: IndexKind::Identity,
        title: if ident.name.is_empty() {
            ident.id.clone()
        } else {
            ident.name.clone()
        },
        body: body_parts
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        trust: TrustState::Verified,
        owning_scope: None,
        updated_at,
    }
}

/// Lower every history entry of a record into per-entry audit documents.
/// Each entry's trust inherits the record's indexed trust state.
#[must_use]
pub fn audit_docs(record: &Record, record_trust: TrustState) -> Vec<IndexDoc> {
    let rec_id = record.envelope.id.as_str();
    let rec_title = record.envelope.title.as_str();
    record
        .envelope
        .history
        .iter()
        .enumerate()
        .map(|(n, entry)| {
            let op = entry
                .ops_summary
                .first()
                .cloned()
                .unwrap_or_else(|| "history".into());
            let mut body_parts: Vec<String> = Vec::new();
            body_parts.push(entry.primary_actor.as_str().to_string());
            for c in &entry.contributors {
                body_parts.push(c.as_str().to_string());
            }
            body_parts.extend(entry.ops_summary.iter().cloned());
            body_parts.push(format!("{} -> {}", entry.from_hash, entry.to_hash));
            IndexDoc {
                id: DocId::Synthetic {
                    kind: IndexKind::Audit,
                    key: format!("{rec_id}#h{n}"),
                },
                kind: IndexKind::Audit,
                title: format!("{op}: {rec_title}"),
                body: body_parts.join("\n"),
                trust: record_trust,
                owning_scope: record.envelope.owning_scope.clone(),
                updated_at: entry.timestamp,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_doc_has_id_and_owners_in_body() {
        let scope = ft_scope::Scope {
            id: "apps/checkout".into(),
            name: "Checkout".into(),
            applies_to_patterns: vec!["apps/checkout/**".into()],
            applies_to: vec![],
            aliases: vec!["checkout".into()],
            codeowners: None,
        };
        let doc = scope_doc(&scope, Utc::now());
        assert_eq!(doc.kind, IndexKind::Scope);
        assert_eq!(
            doc.id,
            DocId::Synthetic {
                kind: IndexKind::Scope,
                key: "apps/checkout".into()
            }
        );
        assert_eq!(doc.trust, TrustState::Verified);
        assert!(doc.body.contains("apps/checkout/**"));
        assert!(doc.body.contains("checkout"));
    }

    #[test]
    fn identity_doc_indexes_emails() {
        let ident = ft_identity::RegisteredIdentity {
            id: "alice".into(),
            name: "Alice".into(),
            kind: ft_identity::IdentityKind::Human,
            emails: vec!["alice@example.com".into()],
            machines: vec![],
            capabilities: ft_identity::PartialCapabilityMatrix::default(),
            status: ft_identity::IdentityStatus::default(),
        };
        let doc = identity_doc(&ident, Utc::now());
        assert_eq!(doc.kind, IndexKind::Identity);
        assert_eq!(
            doc.id,
            DocId::Synthetic {
                kind: IndexKind::Identity,
                key: "alice".into()
            }
        );
        assert_eq!(doc.trust, TrustState::Verified);
        assert!(doc.body.contains("alice@example.com"));
    }

    #[test]
    fn audit_docs_empty_history_is_empty() {
        let record = ft_testkit::make_task().build();
        assert!(record.envelope.history.is_empty());
        assert!(audit_docs(&record, TrustState::Verified).is_empty());
    }

    #[test]
    fn audit_docs_maps_each_entry() {
        let mut record = ft_testkit::make_task().title("Wire reindex").build();
        record.envelope.history.push(ft_core::HistoryEntry {
            merged_via_pr: Some(42),
            timestamp: Utc::now(),
            primary_actor: ft_testkit::make_identity_named("alice"),
            contributors: vec![ft_testkit::make_identity_named("bob")],
            ops_summary: vec!["set status".into(), "set owner".into()],
            ops_count: 2,
            from_hash: "aaaa".into(),
            to_hash: "bbbb".into(),
        });

        let docs = audit_docs(&record, TrustState::Verified);
        assert_eq!(docs.len(), 1);
        let doc = &docs[0];
        assert_eq!(doc.kind, IndexKind::Audit);
        assert_eq!(
            doc.id,
            DocId::Synthetic {
                kind: IndexKind::Audit,
                key: format!("{}#h0", record.envelope.id.as_str()),
            }
        );
        assert_eq!(doc.trust, TrustState::Verified);
        // First ops_summary becomes the title prefix.
        assert!(doc.title.starts_with("set status: "));
        assert!(doc.title.contains("Wire reindex"));
        // Actors and ops appear in the body.
        assert!(doc.body.contains("alice"));
        assert!(doc.body.contains("bob"));
        assert!(doc.body.contains("set owner"));
        assert!(doc.body.contains("aaaa -> bbbb"));
    }
}

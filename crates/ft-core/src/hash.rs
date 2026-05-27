//! Canonical JSON serialization and `state_hash` computation (ADR-0017).
//!
//! The `state_hash` of a record is the lowercase-hex SHA-256 of the
//! canonical-JSON serialization of the record with the `state_hash` and
//! `prev_state_hash` fields elided from the envelope. The chain field
//! (`prev_state_hash`) is wired by `ft-history` from M2; `ft-core` only
//! computes the per-version hash.
//!
//! "Canonical" JSON in Firetrail means:
//! - UTF-8, no BOM
//! - Object keys sorted lexicographically at every level
//! - No insignificant whitespace
//! - `serde_json`'s standard escaping rules
//!
//! The implementation re-parses through `serde_json::Value` and then walks
//! the tree to emit keys in sorted order. This is O(n log n) in record size;
//! records are small (typically <10 KB), so the cost is irrelevant.

use sha2::{Digest, Sha256};

use crate::error::CoreError;
use crate::record::Record;

/// Serialize a value to canonical JSON bytes.
///
/// # Errors
///
/// Propagates `serde_json` errors during the initial serialization.
pub fn canonical_json<T: serde::Serialize>(value: &T) -> Result<Vec<u8>, CoreError> {
    let v: serde_json::Value = serde_json::to_value(value)?;
    let mut buf = Vec::with_capacity(256);
    write_canonical(&mut buf, &v);
    Ok(buf)
}

fn write_canonical(buf: &mut Vec<u8>, v: &serde_json::Value) {
    use serde_json::Value;
    match v {
        Value::Null => buf.extend_from_slice(b"null"),
        Value::Bool(b) => buf.extend_from_slice(if *b { b"true" } else { b"false" }),
        Value::Number(n) => buf.extend_from_slice(n.to_string().as_bytes()),
        Value::String(s) => {
            // Re-use serde_json's escaper for strings.
            let encoded = serde_json::to_string(s).expect("string always serializes");
            buf.extend_from_slice(encoded.as_bytes());
        }
        Value::Array(items) => {
            buf.push(b'[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    buf.push(b',');
                }
                write_canonical(buf, item);
            }
            buf.push(b']');
        }
        Value::Object(map) => {
            // Sort keys lexicographically.
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            buf.push(b'{');
            for (i, k) in keys.iter().enumerate() {
                if i > 0 {
                    buf.push(b',');
                }
                let encoded_key = serde_json::to_string(k).expect("string always serializes");
                buf.extend_from_slice(encoded_key.as_bytes());
                buf.push(b':');
                write_canonical(buf, &map[*k]);
            }
            buf.push(b'}');
        }
    }
}

/// Compute the `state_hash` of a record per ADR-0017.
///
/// The fields `state_hash` and `prev_state_hash` are removed from the
/// envelope before hashing. The result is the lowercase-hex SHA-256 digest of
/// the canonical-JSON form.
///
/// # Errors
///
/// Returns [`CoreError::Serde`] if the record cannot be serialized.
///
/// # Examples
///
/// ```
/// use ft_core::{builder::RecordBuilder, hash::state_hash, Identity, Priority, RecordKind};
///
/// let alice = Identity::new("alice@example.com").unwrap();
/// let mut record = RecordBuilder::new(RecordKind::Task, "demo", alice)
///     .priority(Priority::P2)
///     .build()
///     .unwrap();
/// // The builder set state_hash; recompute to verify determinism.
/// let h1 = state_hash(&record).unwrap();
/// assert_eq!(h1, record.envelope.state_hash);
///
/// // Mutating state_hash does not change the recomputed hash.
/// record.envelope.state_hash = "tampered".into();
/// let h2 = state_hash(&record).unwrap();
/// assert_eq!(h1, h2);
/// ```
pub fn state_hash(record: &Record) -> Result<String, CoreError> {
    let mut value: serde_json::Value = serde_json::to_value(record)?;
    if let Some(envelope) = value.get_mut("envelope").and_then(|e| e.as_object_mut()) {
        envelope.remove("state_hash");
        envelope.remove("prev_state_hash");
    }
    let mut buf = Vec::with_capacity(512);
    write_canonical(&mut buf, &value);
    let digest = Sha256::digest(&buf);
    Ok(hex::encode(digest))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::RecordBuilder;
    use crate::enums::Priority;
    use crate::id::RecordKind;
    use crate::identity::Identity;

    fn alice() -> Identity {
        Identity::new("alice@example.com").unwrap()
    }

    fn sample_task() -> Record {
        RecordBuilder::new(RecordKind::Task, "demo", alice())
            .priority(Priority::P2)
            .build()
            .unwrap()
    }

    #[test]
    fn canonical_json_sorts_object_keys() {
        let v = serde_json::json!({"b": 1, "a": 2, "nested": {"z": 0, "y": 1}});
        let bytes = canonical_json(&v).unwrap();
        let s = std::str::from_utf8(&bytes).unwrap();
        assert_eq!(s, r#"{"a":2,"b":1,"nested":{"y":1,"z":0}}"#);
    }

    #[test]
    fn state_hash_is_deterministic() {
        let r = sample_task();
        let h1 = state_hash(&r).unwrap();
        let h2 = state_hash(&r).unwrap();
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
    }

    #[test]
    fn state_hash_excludes_hash_fields() {
        let mut r = sample_task();
        let h1 = state_hash(&r).unwrap();
        r.envelope.state_hash =
            "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".into();
        r.envelope.prev_state_hash = Some("00".repeat(32));
        let h2 = state_hash(&r).unwrap();
        assert_eq!(h1, h2, "hash must ignore state_hash/prev_state_hash");
    }

    #[test]
    fn state_hash_changes_when_content_changes() {
        let r1 = sample_task();
        let mut r2 = r1.clone();
        r2.envelope.title = "changed".into();
        assert_ne!(state_hash(&r1).unwrap(), state_hash(&r2).unwrap());
    }
}

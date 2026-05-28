//! `graph` op — dependency-graph traversal returning nodes + edges.
//!
//! Mirrors `ft_cli::commands::graph` but flattens the result into the
//! `{ nodes, edges }` shape the GUI's force-directed layout expects. The
//! CLI's nested `BTreeMap<String, Vec<Node>>` is reachable by re-grouping
//! edges by `from` on the client.

use std::collections::HashSet;

use ft_index::{Index, WalkDirection};
use ft_storage::{EmbeddedStorage, Storage as _, StorageFilter};
use serde::{Deserialize, Serialize};

use crate::error::OpsError;
use crate::events::EventBus;
use crate::identity::Identity;
use crate::workspace::Workspace;

/// Walk direction selector.
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum GraphDirectionInput {
    /// Walk to parents / dependencies.
    Up,
    /// Walk to children / dependents.
    Down,
    /// Walk both ways.
    Both,
}

impl GraphDirectionInput {
    fn to_core(self) -> WalkDirection {
        match self {
            Self::Up => WalkDirection::Upstream,
            Self::Down => WalkDirection::Downstream,
            Self::Both => WalkDirection::Both,
        }
    }
}

/// One node in the result graph.
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphNode {
    /// Canonical record id.
    pub id: String,
    /// Record kind (lowercase, e.g. `"task"`).
    pub kind: String,
    /// Title at the time of the walk (empty when unknown).
    pub title: String,
}

/// One edge in the result graph.
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphEdge {
    /// Source id.
    pub from: String,
    /// Target id.
    pub to: String,
    /// Relation kind (e.g. `"parent_epic"`, `"depends_on"`).
    pub kind: String,
    /// 0-based depth from the root.
    pub depth: u32,
}

/// Input for [`graph`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export, rename = "GraphInput"))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphInput {
    /// Root id (full canonical or unambiguous prefix).
    pub id: String,
    /// Walk direction.
    #[serde(default = "default_direction")]
    pub direction: GraphDirectionInput,
    /// Walk depth (minimum 1).
    #[serde(default = "default_depth")]
    pub depth: u32,
    /// Optional client-supplied correlation id.
    #[serde(default)]
    pub request_id: Option<String>,
}

fn default_direction() -> GraphDirectionInput {
    GraphDirectionInput::Both
}
fn default_depth() -> u32 {
    2
}

/// Output of [`graph`].
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-rs", ts(export, rename = "GraphOutput"))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphOutput {
    /// Root id (canonical).
    pub root: String,
    /// Walk depth requested.
    pub depth: u32,
    /// Walk direction (echoed back as lowercase string).
    pub direction: String,
    /// Distinct nodes (root included).
    pub nodes: Vec<GraphNode>,
    /// All edges discovered during the walk.
    pub edges: Vec<GraphEdge>,
    /// Self-describing reason when the walk found no edges.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// `graph` op.
#[allow(clippy::needless_pass_by_value)]
pub fn graph(
    ws: &Workspace,
    _identity: &Identity,
    input: GraphInput,
    _events: &EventBus,
) -> Result<GraphOutput, OpsError> {
    let storage = EmbeddedStorage::open(&ws.root)
        .map_err(|e| OpsError::Internal(anyhow::anyhow!("open storage: {e}")))?;
    let mut index = Index::open(&ws.root)
        .map_err(|e| OpsError::Internal(anyhow::anyhow!("open index: {e}")))?;
    if index.schema_version() == 0 {
        index
            .rebuild_from(&storage)
            .map_err(|e| OpsError::Internal(anyhow::anyhow!("rebuild index: {e}")))?;
    }

    let root = resolve_id(&storage, &input.id)?;
    // Verify root exists before walking.
    let _ = storage.read(&root).map_err(|e| match e {
        ft_storage::StorageError::NotFound(_) => OpsError::not_found("memory", input.id.clone()),
        other => OpsError::Internal(anyhow::anyhow!("read root: {other}")),
    })?;

    let direction = input.direction.to_core();
    let depth = (input.depth.max(1)) as usize;
    let edges = index
        .dependency_walk(&root, direction, depth)
        .map_err(|e| OpsError::Internal(anyhow::anyhow!("dependency walk: {e}")))?;

    let mut node_ids: HashSet<String> = HashSet::new();
    node_ids.insert(root.as_str().to_string());

    let out_edges: Vec<GraphEdge> = edges
        .iter()
        .map(|e| {
            node_ids.insert(e.from.as_str().to_string());
            node_ids.insert(e.to.as_str().to_string());
            GraphEdge {
                from: e.from.as_str().to_string(),
                to: e.to.as_str().to_string(),
                kind: serde_json::to_value(e.kind)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_else(|| format!("{:?}", e.kind)),
                depth: e.depth,
            }
        })
        .collect();

    let mut nodes: Vec<GraphNode> = Vec::with_capacity(node_ids.len());
    for id_str in node_ids {
        let (kind, title) = match ft_core::RecordId::from_string(id_str.clone()) {
            Ok(rid) => match storage.read(&rid) {
                Ok(r) => (
                    format!("{:?}", r.envelope.kind).to_ascii_lowercase(),
                    r.envelope.title,
                ),
                Err(_) => (String::new(), String::new()),
            },
            Err(_) => (String::new(), String::new()),
        };
        nodes.push(GraphNode {
            id: id_str,
            kind,
            title,
        });
    }
    nodes.sort_by(|a, b| a.id.cmp(&b.id));

    let reason = if out_edges.is_empty() {
        Some("no relations involve this record".into())
    } else {
        None
    };

    Ok(GraphOutput {
        root: root.as_str().to_string(),
        depth: input.depth,
        direction: format!("{:?}", input.direction).to_ascii_lowercase(),
        nodes,
        edges: out_edges,
        reason,
    })
}

fn resolve_id(storage: &EmbeddedStorage, raw: &str) -> Result<ft_core::RecordId, OpsError> {
    if let Ok(id) = ft_core::RecordId::from_string(raw.to_string()) {
        return if storage.read(&id).is_ok() {
            Ok(id)
        } else {
            Err(OpsError::not_found("memory", raw.to_string()))
        };
    }
    let candidates = storage
        .list(&StorageFilter::default())
        .map_err(|e| OpsError::Internal(anyhow::anyhow!("scan storage: {e}")))?;
    match ft_core::resolve_prefix(raw, &candidates) {
        Ok(id) => Ok(id),
        Err(ft_core::ResolveError::Empty) => Err(OpsError::validation("id", "empty record id")),
        Err(ft_core::ResolveError::EmptyHexPrefix(k)) => Err(OpsError::validation(
            "id",
            format!("hex prefix required after kind tag `{k}`"),
        )),
        Err(ft_core::ResolveError::Unknown(_)) => {
            Err(OpsError::not_found("memory", raw.to_string()))
        }
        Err(ft_core::ResolveError::Ambiguous { matches, .. }) => Err(OpsError::Conflict {
            reason: format!("`{raw}` is ambiguous; matches {} records", matches.len()),
        }),
    }
}

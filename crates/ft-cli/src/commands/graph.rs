//! `firetrail graph <id>` — ASCII tree of dependency walk.

use std::collections::BTreeMap;

use ft_index::WalkDirection;
use serde::Serialize;

use crate::cli::{GlobalOpts, GraphArgs, GraphDirArg};
use crate::commands::CommandOutcome;
use crate::context::WorkCtx;
use crate::error::CliError;

const COMMAND: &str = "graph";

/// Entry point.
pub fn run(args: &GraphArgs, global: &GlobalOpts) -> Result<CommandOutcome, CliError> {
    let ctx = WorkCtx::open(COMMAND, global.workspace.as_deref())?;
    let warnings = ctx.warnings.clone();
    let root = ctx.resolve_id(&args.id)?;
    // Verify the record exists before walking.
    let _ = ctx.read_record(&root)?;

    let direction = match args.direction {
        GraphDirArg::Up => WalkDirection::Upstream,
        GraphDirArg::Down => WalkDirection::Downstream,
        GraphDirArg::Both => WalkDirection::Both,
    };
    let depth = args.depth.max(1) as usize;
    let edges = ctx
        .index
        .dependency_walk(&root, direction, depth)
        .map_err(|e| CliError::internal(COMMAND, e))?;

    // Group children by their parent id.
    let mut children: BTreeMap<String, Vec<GraphNode>> = BTreeMap::new();
    for e in &edges {
        children
            .entry(e.from.as_str().to_string())
            .or_default()
            .push(GraphNode {
                id: e.to.as_str().to_string(),
                kind: serde_json::to_value(e.kind)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_else(|| format!("{:?}", e.kind)),
                depth: e.depth,
            });
    }
    // Sort children for stable snapshots.
    for v in children.values_mut() {
        v.sort_by(|a, b| (a.kind.clone(), a.id.clone()).cmp(&(b.kind.clone(), b.id.clone())));
    }

    let reason = if children.is_empty() {
        Some("no relations involve this record".to_string())
    } else {
        None
    };

    Ok(CommandOutcome::Graph(GraphOutcome {
        root: root.as_str().to_string(),
        depth: args.depth,
        direction: format!("{:?}", args.direction).to_lowercase(),
        edges: children,
        reason,
        warnings,
    }))
}

#[derive(Debug, Clone, Serialize)]
pub struct GraphNode {
    pub id: String,
    pub kind: String,
    pub depth: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct GraphOutcome {
    pub root: String,
    pub depth: u32,
    pub direction: String,
    /// `parent_id -> [(kind, child_id, depth)]`.
    pub edges: BTreeMap<String, Vec<GraphNode>>,
    /// Self-describing reason when `edges` is empty. Disambiguates "no
    /// relations exist" from a query bug, per firetrail-1sg.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reason: Option<String>,
    /// Non-fatal warnings (e.g. index auto-rebuild on open).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<String>,
}

impl GraphOutcome {
    pub fn markdown(&self) -> String {
        let mut s = format!(
            "# graph `{}` (depth={}, direction={})\n\n",
            self.root, self.depth, self.direction
        );
        s.push_str(&self.root);
        s.push('\n');
        let mut visited = std::collections::HashSet::new();
        visited.insert(self.root.clone());
        self.render(&self.root, &mut s, "", &mut visited);
        if let Some(reason) = &self.reason {
            use std::fmt::Write as _;
            let _ = write!(s, "\n_{reason}_\n");
        }
        s
    }

    fn render(
        &self,
        from: &str,
        s: &mut String,
        prefix: &str,
        visited: &mut std::collections::HashSet<String>,
    ) {
        use std::fmt::Write as _;
        let Some(children) = self.edges.get(from) else {
            return;
        };
        let last = children.len().saturating_sub(1);
        for (idx, node) in children.iter().enumerate() {
            let connector = if idx == last {
                "└── "
            } else {
                "├── "
            };
            s.push_str(prefix);
            s.push_str(connector);
            let _ = writeln!(s, "[{}] {}", node.kind, node.id);
            let extension = if idx == last { "    " } else { "│   " };
            let next_prefix = format!("{prefix}{extension}");
            if visited.insert(node.id.clone()) {
                self.render(&node.id, s, &next_prefix, visited);
            }
        }
    }

    pub fn quiet_line(&self) -> String {
        let n: usize = self.edges.values().map(Vec::len).sum();
        format!("graph: {n} edge(s) from {}", self.root)
    }
}

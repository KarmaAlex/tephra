//! What the window remembers between runs, saved in `.tephra/workspace.json` inside the vault.
//!
//! It mirrors Obsidian's `.obsidian/workspace.json` (which stays read-only): a tree of `split`
//! and `tabs` nodes, whose `leaf` children are the open notes and views, plus the leaf that had
//! the focus. Only what this app has a feature for is kept — pane widths, per-view state (the
//! file explorer's search box, the outline's follow-cursor switch), `lastOpenFiles` and the
//! ribbon's hidden items have no counterpart here. Unknown keys in a file are ignored, so an
//! older or hand-edited file still loads.
//!
//! The parsing and writing here are pure (no vault, no window), so they are unit-tested in this
//! file; opening the notes is `Workspace::restore_layout`.

use std::path::Path;

use serde_json::{json, Value};

use crate::editor::ViewMode;

/// How a split arranges its children: side by side (`horizontal`) or one above the other
/// (`vertical`), named as Obsidian names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Horizontal,
    Vertical,
}

impl Direction {
    pub fn id(self) -> &'static str {
        match self {
            Direction::Horizontal => "horizontal",
            Direction::Vertical => "vertical",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "horizontal" => Some(Direction::Horizontal),
            "vertical" => Some(Direction::Vertical),
            _ => None,
        }
    }
}

/// What a leaf shows.
#[derive(Clone, Debug, PartialEq)]
pub enum Leaf {
    /// A note (or other text file), with the way it was shown.
    File { path: String, mode: ViewMode },
    /// A tab a plugin opened: its view id (`app:image`, `graph:graph`) and the file it shows.
    View { view: String, arg: Option<String> },
}

/// A node of the saved arrangement: the tabs of one pane, or a split of them.
#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    Tabs { active: usize, leaves: Vec<Leaf> },
    Split { direction: Direction, children: Vec<Node> },
}

/// The saved workspace.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Saved {
    /// The arrangement, when anything was open.
    pub main: Option<Node>,
    /// The pane that had the focus (its index in layout order).
    pub active: usize,
}

impl Node {
    /// The leaves in this node, in reading order.
    pub fn leaves(&self) -> Vec<&Leaf> {
        match self {
            Node::Tabs { leaves, .. } => leaves.iter().collect(),
            Node::Split { children, .. } => children.iter().flat_map(Node::leaves).collect(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.leaves().is_empty()
    }
}

/// The JSON for a node.
pub fn to_json(node: &Node) -> Value {
    match node {
        Node::Tabs { active, leaves } => json!({
            "type": "tabs",
            "active": active,
            "children": leaves.iter().map(leaf_json).collect::<Vec<_>>(),
        }),
        Node::Split { direction, children } => json!({
            "type": "split",
            "direction": direction.id(),
            "children": children.iter().map(to_json).collect::<Vec<_>>(),
        }),
    }
}

fn leaf_json(leaf: &Leaf) -> Value {
    match leaf {
        Leaf::File { path, mode } => json!({
            "type": "leaf",
            "state": { "type": "markdown", "file": path, "mode": mode.id() },
        }),
        Leaf::View { view, arg } => json!({
            "type": "leaf",
            "state": { "type": "view", "view": view, "arg": arg },
        }),
    }
}

/// A node from JSON; None when there is nothing open in it. Splits with one child collapse.
pub fn from_json(v: &Value) -> Option<Node> {
    match v.get("type").and_then(Value::as_str) {
        Some("split") => {
            let direction = v.get("direction").and_then(Value::as_str).and_then(Direction::from_id).unwrap_or(Direction::Vertical);
            let mut children: Vec<Node> = v.get("children").and_then(Value::as_array).map(|a| a.iter().filter_map(from_json).collect()).unwrap_or_default();
            match children.len() {
                0 => None,
                1 => children.pop(),
                _ => Some(Node::Split { direction, children }),
            }
        }
        // a leaf on its own: a pane with one tab
        Some("leaf") => leaf_from_json(v).map(|leaf| Node::Tabs { active: 0, leaves: vec![leaf] }),
        _ => {
            let leaves: Vec<Leaf> = v.get("children").and_then(Value::as_array).map(|a| a.iter().filter_map(leaf_from_json).collect()).unwrap_or_default();
            if leaves.is_empty() {
                return None;
            }
            let active = v.get("active").and_then(Value::as_u64).unwrap_or(0) as usize;
            Some(Node::Tabs { active: active.min(leaves.len() - 1), leaves })
        }
    }
}

fn leaf_from_json(v: &Value) -> Option<Leaf> {
    // ours keeps the state flat; Obsidian wraps the item's own state one level down
    let state = v.get("state")?;
    let inner = state.get("state").unwrap_or(state);
    let text = |k: &str| inner.get(k).or_else(|| state.get(k)).and_then(Value::as_str).map(String::from);
    match state.get("type").and_then(Value::as_str).unwrap_or("markdown") {
        "markdown" | "file" => Some(Leaf::File {
            path: text("file")?,
            mode: text("mode").as_deref().and_then(ViewMode::from_id).unwrap_or_default(),
        }),
        "view" => Some(Leaf::View { view: text("view")?, arg: text("arg").or_else(|| text("file")) }),
        // another app's view type (a file explorer leaf, say): nothing to reopen here
        _ => None,
    }
}

/// True when `path` (vault-relative, as saved) still names something inside the vault.
pub fn in_vault(root: &Path, path: &str) -> bool {
    let p = Path::new(path);
    !p.is_absolute() && !path.split(['/', '\\']).any(|c| c.is_empty() || c == "..") && root.join(p).exists()
}

/// Drops the leaves whose file is gone (deleted, or moved while the app was closed) and the tabs
/// nodes that end up empty; a split whose children are gone becomes empty too.
pub fn retain_existing(node: &mut Node, root: &Path) {
    match node {
        Node::Tabs { active, leaves } => {
            leaves.retain(|leaf| match leaf {
                Leaf::File { path, .. } => in_vault(root, path),
                Leaf::View { arg: Some(arg), .. } => in_vault(root, arg),
                Leaf::View { arg: None, .. } => true,
            });
            *active = (*active).min(leaves.len().saturating_sub(1));
        }
        Node::Split { children, .. } => {
            for c in children.iter_mut() {
                retain_existing(c, root);
            }
            children.retain(|c| !c.is_empty());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_json() -> Value {
        serde_json::from_str(
            r#"{
              "type": "split",
              "direction": "horizontal",
              "children": [
                { "type": "tabs", "active": 1, "children": [
                  { "type": "leaf", "state": { "type": "markdown", "file": "A.md", "mode": "live" } },
                  { "type": "leaf", "state": { "type": "markdown", "file": "B.md", "mode": "source" } }
                ]},
                { "type": "tabs", "children": [
                  { "type": "leaf", "state": { "type": "view", "view": "app:image", "arg": "Images/p.png" } }
                ]}
              ]
            }"#,
        )
        .unwrap()
    }

    #[test]
    fn round_trips_a_saved_arrangement() {
        let node = from_json(&fixture_json()).expect("parses");
        assert_eq!(
            node,
            Node::Split {
                direction: Direction::Horizontal,
                children: vec![
                    Node::Tabs {
                        active: 1,
                        leaves: vec![
                            Leaf::File { path: "A.md".into(), mode: ViewMode::LivePreview },
                            Leaf::File { path: "B.md".into(), mode: ViewMode::Source },
                        ],
                    },
                    Node::Tabs { active: 0, leaves: vec![Leaf::View { view: "app:image".into(), arg: Some("Images/p.png".into()) }] },
                ],
            }
        );
        // writing it and reading it back gives the same thing
        assert_eq!(from_json(&to_json(&node)), Some(node));
    }

    #[test]
    fn reads_obsidians_nested_leaf_state() {
        let v: Value = serde_json::from_str(
            r#"{"type": "tabs", "children": [
                 {"type": "leaf", "state": {"type": "markdown", "state": {"file": "A.md", "mode": "source"}}}
               ]}"#,
        )
        .unwrap();
        assert_eq!(
            from_json(&v),
            Some(Node::Tabs { active: 0, leaves: vec![Leaf::File { path: "A.md".into(), mode: ViewMode::Source }] })
        );
    }

    #[test]
    fn unknown_and_empty_nodes_are_dropped() {
        // a file-explorer leaf has nothing to reopen here; with nothing left the node is None
        let v: Value = serde_json::from_str(r#"{"type": "tabs", "children": [{"type": "leaf", "state": {"type": "file-explorer", "state": {}}}]}"#).unwrap();
        assert_eq!(from_json(&v), None);
        assert_eq!(from_json(&json!({"type": "tabs", "children": []})), None);
        assert_eq!(from_json(&json!({"type": "split", "children": []})), None);
        // junk from another app's file, or none at all
        assert_eq!(from_json(&json!(null)), None);
        assert_eq!(from_json(&json!({"type": "leaf"})), None);
    }

    #[test]
    fn one_child_splits_collapse_and_active_is_clamped() {
        let v = json!({"type": "split", "children": [{"type": "tabs", "children": [
            {"type": "leaf", "state": {"type": "markdown", "file": "A.md"}}
        ]}]});
        assert_eq!(from_json(&v), Some(Node::Tabs { active: 0, leaves: vec![Leaf::File { path: "A.md".into(), mode: ViewMode::LivePreview }] }));
        // an `active` past the last tab falls back to the last one
        let v = json!({"type": "tabs", "active": 9, "children": [{"type": "leaf", "state": {"type": "markdown", "file": "A.md"}}]});
        assert_eq!(from_json(&v).and_then(|n| match n { Node::Tabs { active, .. } => Some(active), _ => None }), Some(0));
    }

    #[test]
    fn gone_files_are_dropped_and_empty_panes_go_away() {
        let dir = std::env::temp_dir().join(format!("tephra-ws-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("Images")).unwrap();
        std::fs::write(dir.join("A.md"), "a").unwrap();
        std::fs::write(dir.join("Images/p.png"), "p").unwrap();
        let mut node = from_json(&fixture_json()).unwrap();
        retain_existing(&mut node, &dir);
        // B.md was never there; the image pane stays
        assert_eq!(
            node,
            Node::Split {
                direction: Direction::Horizontal,
                children: vec![
                    Node::Tabs { active: 0, leaves: vec![Leaf::File { path: "A.md".into(), mode: ViewMode::LivePreview }] },
                    Node::Tabs { active: 0, leaves: vec![Leaf::View { view: "app:image".into(), arg: Some("Images/p.png".into()) }] },
                ],
            }
        );
        // a pane whose only note is gone disappears from the split
        let mut node = Node::Split {
            direction: Direction::Vertical,
            children: vec![
                Node::Tabs { active: 0, leaves: vec![Leaf::File { path: "A.md".into(), mode: ViewMode::LivePreview }] },
                Node::Tabs { active: 0, leaves: vec![Leaf::File { path: "gone.md".into(), mode: ViewMode::LivePreview }] },
            ],
        };
        retain_existing(&mut node, &dir);
        assert_eq!(node.leaves().len(), 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn paths_outside_the_vault_are_refused() {
        let root = std::env::temp_dir().join(format!("tephra-ws-paths-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("A.md"), "a").unwrap();
        assert!(in_vault(&root, "A.md"));
        assert!(!in_vault(&root, "B.md")); // not there
        assert!(!in_vault(&root, "/etc/passwd")); // absolute
        assert!(!in_vault(&root, "../A.md")); // escapes the vault
        assert!(!in_vault(&root, "sub//A.md")); // not a path we write
        assert!(!in_vault(&root, ""));
        std::fs::remove_dir_all(&root).ok();
    }
}

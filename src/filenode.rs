use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct FileNode {
    pub id: String,
    pub parent: Option<String>,
    #[serde(default)]
    pub children: HashSet<String>,
}

impl FileNode {
    /// Creates a new FileNode with an explicit parent and a trace.
    /// The trace should represent the path from the root to the current node.
    pub fn new(id: String, parent: Option<String>, children: HashSet<String>) -> Self {
        Self {
            id: id.clone(),
            parent,
            children,
        }
    }

    /// Convenience function to create a root FileNode.
    /// For a root, the trace is empty.
    pub fn new_root(id: String, children: HashSet<String>) -> Self {
        Self {
            id,
            parent: None,
            children,
        }
    }

    /// Creates a new FileNode as a child of an existing node,
    /// automatically appending the parent's identifier to the trace.
    pub fn new_with_parent(id: String, parent_id: String, children: HashSet<String>) -> Self {
        Self {
            id,
            parent: Some(parent_id),
            children,
        }
    }

    /// Checks if this node is a root node (i.e., it has no parent).
    pub fn is_root(&self) -> bool {
        self.parent.is_none()
    }
}

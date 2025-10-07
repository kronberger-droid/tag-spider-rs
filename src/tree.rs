use crate::filenode::FileNode;
use anyhow::{bail, Context, Result};
use async_recursion::async_recursion;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
};
use thirtyfour::{prelude::ElementQueryable, By, WebDriver, WebElement};

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct FileTree {
    pub nodes: HashMap<String, FileNode>,
    #[serde(default)]
    pub root: FileNode,
}

impl FileTree {
    pub fn new(root_id: String) -> Self {
        Self {
            nodes: HashMap::new(),
            root: FileNode::new_root(root_id, HashSet::new()),
        }
    }

    pub async fn build_tree(driver: &WebDriver) -> Result<FileTree> {
        let mut tree = FileTree::new(String::new());

        // Find the top-level tree container and all items
        let container = driver
            .find(By::Css("[role='tree']"))
            .await
            .context("Did not find a file tree in current view!")?;
        let items = container
            .find_all(By::Css("[role='treeitem']"))
            .await
            .context("Could not find any treeitems in the current tree!")?;

        // Insert each item into the tree, capturing its parent
        for item in &items {
            // The aria-labelledby attribute typically stores a unique ID or label
            if let Some(id) = item
                .attr("aria-labelledby")
                .await
                .context("Could not get attribute 'aria-labelledby'")?
            {
                let parent_id = match find_parent_id(item).await? {
                    Some(ref pid) => {
                        // Check if parent node already exists
                        if let Some(parent_node) = tree.nodes.get_mut(pid) {
                            parent_node.children.insert(id.clone());
                        } else {
                            // Parent node doesn't exist yet, create it
                            let mut children = HashSet::new();
                            children.insert(id.clone());

                            let parent_node = FileNode {
                                id: pid.clone(),
                                parent: None,
                                children,
                            };

                            tree.nodes.insert(pid.clone(), parent_node);
                        }

                        Some(pid.clone()) // Set the current node's parent
                    }
                    None => None, // No parent, so this is a root
                };

                // Finally, insert the current node
                let node = FileNode {
                    id: id.clone(),
                    parent: parent_id,
                    children: HashSet::new(),
                };

                tree.nodes.insert(id, node);
            }
        }

        // Validate and return
        tree.check_root()?;
        Ok(tree)
    }

    /// Validate the consistency of the tree.
    pub fn check_root(&self) -> Result<FileNode> {
        let mut root_count = 0;
        let mut roots = Vec::new();

        for node in self.nodes.values() {
            if let Some(pid) = &node.parent {
                if !self.nodes.contains_key(pid) {
                    bail!("Parent node {} not found for {}", pid, node.id);
                }
            } else {
                roots.push(node.clone());
                root_count += 1;
            }
        }
        if root_count == 1 {
            let root = roots
                .first()
                .expect("If root_count is 1, there should be at least one entry in roots vec!");

            Ok(root.clone())
        } else if root_count == 0 {
            bail!("Found no root in this FileTree")
        } else {
            bail!(format!(
                "We would expect only one root in a given FileTree not: {}",
                root_count
            ))
        }
    }

    pub fn from_json_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let data = fs::read_to_string(path)?;
        let mut tree: FileTree =
            serde_json::from_str(&data).context("could not read json file:")?;
        if let Ok(root) = tree.check_root() {
            tree.root = root;
        };
        Ok(tree)
    }

    pub fn to_json_file<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        fs::write(path, json)?;
        Ok(())
    }
}

/// Recursively climbs up the DOM to find a parent tree item.
/// Returns the `aria-labelledby` ID of the parent if found.
#[async_recursion]
async fn find_parent_id(item: &WebElement) -> Result<Option<String>> {
    // Go one level up
    let parent = match item.query(By::XPath("..")).first().await {
        Ok(el) => el,
        Err(_) => return Ok(None), // No parent => root
    };

    // If the parent is itself a treeitem, we found our parent
    if let Some(role) = parent.attr("role").await? {
        if role == "treeitem" {
            // Return the parent's aria-labelledby as the parent ID
            return Ok(parent.attr("aria-labelledby").await?);
        }
    }

    // Otherwise, keep climbing up
    find_parent_id(&parent).await
}

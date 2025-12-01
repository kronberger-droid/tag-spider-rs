use std::sync::RwLock;
use std::time::Duration;

use crate::{auth::{is_relogin_dialog_present, handle_relogin_dialog}, dom_constants::*, lexer::Lexer, tree::FileTree};
use anyhow::{Context, Result};
use async_recursion::async_recursion;
use thirtyfour::{prelude::*, support, WebDriver};
use tokio::fs;

pub struct Spider {
    pub driver: WebDriver,
    pub file_tree: RwLock<FileTree>,
}

impl Spider {
    pub async fn new<C>(capabilities: C, url: &str, file_tree: FileTree) -> Result<Self>
    where
        C: Into<Capabilities>,
    {
        let driver = WebDriver::new("http://localhost:4444", capabilities)
            .await
            .context("Failed to create WebDriver")?;
        driver.get(url).await.context("Failed to navigate to URL")?;

        Ok(Self {
            driver,
            file_tree: RwLock::new(file_tree)
        })
    }

    #[async_recursion]
    pub async fn find_treeitem(&self, id: &str) -> Result<WebElement> {
        let parent_id_opt = {
            let file_tree = self.file_tree.read().unwrap();
            let current_node = file_tree.nodes.get(id).context(format!(
                "Could not find node with given id in the filetree! {}",
                id
            ))?;
            current_node.parent.clone()
        };

        if let Some(parent_id) = parent_id_opt {
            // Check if already expanded before toggling
            let expanded = self
                .find_treeitem(&parent_id)
                .await?
                .attr("aria-expanded")
                .await?;

            if expanded != Some("true".to_string()) {
                self.click_treeitem_toggle(&parent_id).await?;
            }
        }

        // Now attempt to find the current node
        let selector = treeitem_selector(id);

        let treeitem = self.driver.find(By::Css(&selector)).await
            .context(format!("Could not find treeitem with id: {}", id))?;

        treeitem.scroll_into_view().await
            .context("Could not scroll treeitem into view")?;

        Ok(treeitem)
    }

    pub async fn click_treeitem(&self, id: &str) -> Result<()> {
        let treeitem = self.find_treeitem(id).await?;
        let treeitem_header = treeitem
            .find(By::ClassName(NODE_HEADER_LABEL_WRAPPER))
            .await
            .context("Could not find treeitem header!")?;
        treeitem_header
            .click()
            .await
            .context("Treeitem not clickable!")?;

        Ok(())
    }

    pub async fn click_treeitem_toggle(&self, id: &str) -> Result<()> {
        let treeitem = self.find_treeitem(id).await?;
        let treeitem_toggle = treeitem
            .find(By::Css(NODE_HEADER_CHEVRON_FULL))
            .await
            .context("Could not find toggle button in this element!")?;

        treeitem_toggle
            .click()
            .await
            .context("Could not click the toggle button!")?;

        Ok(())
    }

    /// Retry wrapper that handles relogin dialogs automatically
    pub async fn retry_with_relogin<F, Fut, T>(
        &self,
        operation: F,
        max_retries: usize,
    ) -> Result<T>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        let mut last_error = None;

        for attempt in 0..=max_retries {
            // Check for relogin dialog before operation
            if is_relogin_dialog_present(&self.driver).await {
                println!("Relogin dialog detected before operation attempt {}", attempt + 1);
                match handle_relogin_dialog(&self.driver).await {
                    Ok(true) => {
                        println!("Relogin successful, continuing...");
                        support::sleep(Duration::from_secs(2)).await;
                    },
                    Ok(false) | Err(_) => {
                        return Err(anyhow::anyhow!("Relogin failed"));
                    }
                }
            }

            // Attempt operation
            match operation().await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    let error_msg = e.to_string();

                    if error_msg.contains("neos-ReloginDialog") ||
                       error_msg.contains("element click intercepted") ||
                       error_msg.contains("ElementClickInterceptedError") {

                        if attempt < max_retries {
                            println!("Operation failed due to relogin (attempt {})", attempt + 1);
                            match handle_relogin_dialog(&self.driver).await {
                                Ok(true) => {
                                    support::sleep(Duration::from_secs(2)).await;
                                    continue;
                                },
                                _ => {
                                    support::sleep(Duration::from_secs(1)).await;
                                    continue;
                                }
                            }
                        }
                    }

                    last_error = Some(e);
                    if attempt < max_retries {
                        println!("Operation failed (attempt {}), retrying...", attempt + 1);
                        support::sleep(Duration::from_secs(2)).await;
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("All retries failed")))
    }

    pub async fn wait_content_load(&self, timeout: Duration) -> Result<()> {
        wait_for_loading(&self.driver, timeout).await
    }

    /// Check if a treeitem is expandable (has chevron)
    pub async fn is_treeitem_expandable(&self, id: &str) -> Result<bool> {
        let selector = treeitem_selector(id);
        let folder_element = self.driver.find(By::Css(&selector)).await
            .context(format!("Could not find folder element '{id}'"))?;

        let chevron_exists = folder_element
            .find(By::Css(NODE_HEADER_CHEVRON))
            .await
            .is_ok();

        Ok(chevron_exists)
    }

    /// Expand a treeitem if it's not already expanded
    pub async fn expand_treeitem_if_needed(&self, id: &str) -> Result<()> {
        self.retry_with_relogin(|| async {
            let treeitem = self.find_treeitem(id).await?;

            let expanded = treeitem.attr(ATTR_ARIA_EXPANDED).await?;
            if expanded != Some("true".to_string()) {
                self.click_treeitem_toggle(id).await?;
                support::sleep(Duration::from_secs(1)).await;
            }
            Ok(())
        }, 3).await
    }

    /// Get immediate children of a treeitem
    /// Uses file_tree as cache first, falls back to DOM traversal
    pub async fn get_treeitem_children(&self, id: &str) -> Result<Vec<String>> {
        println!("Getting children for folder: {id}");

        // Check if expandable
        if !self.is_treeitem_expandable(id).await? {
            println!("Folder {id} is not expandable (no chevron)");
            return Ok(Vec::new());
        }

        // Expand the folder
        self.expand_treeitem_if_needed(id).await?;
        support::sleep(Duration::from_millis(1000)).await;

        // STRATEGY 1: Try file_tree cache first
        {
            let file_tree = self.file_tree.read().unwrap();
            if let Some(node) = file_tree.nodes.get(id) {
                if !node.children.is_empty() {
                    let children: Vec<String> = node.children.iter().cloned().collect();
                    println!("Found {} children in file_tree cache", children.len());
                    return Ok(children);
                }
            }
        }

        // STRATEGY 2: Fallback to DOM traversal
        println!("No children in cache, using DOM traversal...");

        let parent_selector = treeitem_selector(id);
        let parent_element = self.driver.find(By::Css(&parent_selector)).await
            .context("Could not find parent folder element")?;

        let contents_divs = parent_element
            .find_all(By::Css(&format!("div.{}", NODE_CONTENTS)))
            .await?;

        let mut child_ids = Vec::new();

        for contents_div in contents_divs {
            let child_treeitems = contents_div
                .find_all(By::Css(TREEITEM_ROLE))
                .await?;

            for child in child_treeitems {
                if let Some(id) = child.attr(ATTR_ARIA_LABELLEDBY).await? {
                    child_ids.push(id);
                }
            }
        }

        // Fallback method if primary fails
        if child_ids.is_empty() {
            println!("Primary method failed, trying fallback...");

            let all_items = self.driver.find_all(By::Css(TREEITEM_ROLE)).await?;
            let mut found_parent = false;
            let mut parent_level: Option<i32> = None;

            for item in all_items {
                if let Some(item_id) = item.attr(ATTR_ARIA_LABELLEDBY).await? {
                    if item_id == id {
                        found_parent = true;
                        if let Some(level_str) = item.attr(ATTR_ARIA_LEVEL).await? {
                            parent_level = level_str.parse().ok();
                        }
                        continue;
                    }

                    if found_parent {
                        if let Some(parent_lvl) = parent_level {
                            if let Some(current_level_str) = item.attr(ATTR_ARIA_LEVEL).await? {
                                if let Ok(current_level) = current_level_str.parse::<i32>() {
                                    if current_level == parent_lvl + 1 {
                                        child_ids.push(item_id);
                                    } else if current_level <= parent_lvl {
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // De-duplicate results (fixes the duplicate-creating bug)
        child_ids.sort();
        child_ids.dedup();

        println!("Total children found (after dedup): {}", child_ids.len());

        // Update cache with discovered children
        {
            let mut file_tree = self.file_tree.write().unwrap();

            // Update parent node's children
            if let Some(parent_node) = file_tree.nodes.get_mut(id) {
                parent_node.children = child_ids.iter().cloned().collect();
            }

            // Add child nodes to cache if they don't exist
            for child_id in &child_ids {
                file_tree.nodes.entry(child_id.clone()).or_insert_with(|| {
                    use crate::filenode::FileNode;
                    use std::collections::HashSet;
                    FileNode {
                        id: child_id.clone(),
                        parent: Some(id.to_string()),
                        children: HashSet::new(),
                    }
                });
            }

            println!("Updated cache with {} children for {}", child_ids.len(), id);
        }

        Ok(child_ids)
    }

    /// Get all descendants of a treeitem up to max_depth
    #[async_recursion]
    pub async fn get_all_descendants(
        &self,
        id: &str,
        max_depth: usize,
        current_depth: usize,
    ) -> Result<Vec<String>> {
        let mut all_descendants = Vec::new();

        if current_depth >= max_depth {
            println!("  Reached maximum depth {max_depth} for folder: {id}");
            return Ok(all_descendants);
        }

        println!("  Traversing folder at depth {current_depth}: {id}");

        let children = self.get_treeitem_children(id).await?;

        for child_id in children {
            all_descendants.push(child_id.clone());
            println!("    Added child: {child_id}");

            match self.is_treeitem_expandable(&child_id).await {
                Ok(true) => {
                    match self.get_treeitem_children(&child_id).await {
                        Ok(grandchildren) => {
                            if !grandchildren.is_empty() {
                                println!(
                                    "    Child {} has {} grandchildren, recursing...",
                                    child_id,
                                    grandchildren.len()
                                );
                                let descendants = self.get_all_descendants(
                                    &child_id,
                                    max_depth,
                                    current_depth + 1
                                ).await?;
                                all_descendants.extend(descendants);
                            }
                        }
                        Err(e) => {
                            println!("    Failed to get children for {child_id}: {e}");
                        }
                    }
                }
                Ok(false) => {
                    println!("    Child {child_id} is a leaf node");
                }
                Err(e) => {
                    println!("    Could not check if {child_id} is expandable: {e}");
                }
            }

            support::sleep(Duration::from_millis(300)).await;
        }

        println!(
            "  Found {} total descendants for folder: {}",
            all_descendants.len(),
            id
        );
        Ok(all_descendants)
    }

    /// Find content context (main page or iframe)
    pub async fn find_content_context(&self) -> Result<bool> {
        println!("  Looking for content in main context...");

        let main_containers = self.driver
            .find_all(By::Css(DYNAMIC_CONTENT_CONTAINER))
            .await?;
        let main_articles = self.driver
            .find_all(By::Css(FUSION_TUTORIALS))
            .await?;

        if !main_containers.is_empty() || !main_articles.is_empty() {
            println!("  Found content in main context");
            return Ok(false); // Not in iframe
        }

        // Try iframes
        let iframes = self.driver.find_all(By::Tag("iframe")).await?;
        println!("  Found {} iframes on the page", iframes.len());

        for i in 0..iframes.len() {
            println!("  Attempting to enter iframe {}...", i);
            match self.driver.enter_frame(i as u16).await {
                Ok(_) => {
                    println!("  Successfully entered iframe {}", i);

                    let iframe_containers = self.driver
                        .find_all(By::Css(DYNAMIC_CONTENT_CONTAINER))
                        .await?;
                    let iframe_articles = self.driver
                        .find_all(By::Css(FUSION_TUTORIALS))
                        .await?;

                    if !iframe_containers.is_empty() || !iframe_articles.is_empty() {
                        println!("  Found content in iframe {}", i);
                        return Ok(true); // In iframe
                    }

                    self.driver.enter_default_frame().await?;
                }
                Err(e) => {
                    println!("  Failed to enter iframe {}: {}", i, e);
                }
            }
        }

        println!("  No content found in any context");
        Ok(false)
    }

    /// Extract breadcrumb path from current page
    pub async fn extract_breadcrumb_path(&self) -> Result<String> {
        let breadcrumbs = self.driver
            .find_all(By::Css(
                ".neos-breadcrumb a, .breadcrumb a, [class*='breadcrumb'] a",
            ))
            .await?;

        if !breadcrumbs.is_empty() {
            let mut path_parts = Vec::new();
            for breadcrumb in breadcrumbs {
                if let Ok(text) = breadcrumb.text().await {
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        path_parts.push(trimmed.to_string());
                    }
                }
            }
            if !path_parts.is_empty() {
                return Ok(path_parts.join(" > "));
            }
        }

        if let Ok(title) = self.driver.title().await {
            return Ok(title);
        }

        Ok("Unknown Path".to_string())
    }

    /// Save the current file_tree cache to disk
    pub async fn save_cache(&self) -> Result<()> {
        let cache_path = "resources/tree.json";

        // Ensure the resources directory exists
        if let Some(parent) = std::path::Path::new(cache_path).parent() {
            fs::create_dir_all(parent).await
                .context("Failed to create resources directory")?;
        }

        let file_tree = self.file_tree.read().unwrap();
        let json = serde_json::to_string_pretty(&*file_tree)
            .context("Failed to serialize file_tree to JSON")?;

        fs::write(cache_path, json)
            .await
            .context("Failed to write cache to disk")?;

        println!("Cache saved to {}", cache_path);
        Ok(())
    }

    /// Refresh the entire tree cache by performing a full DOM scan
    /// This rebuilds the cache from scratch by traversing all visible nodes
    pub async fn refresh_tree_cache(&self, root_id: &str, max_depth: usize) -> Result<()> {
        println!("Refreshing entire tree cache starting from {}", root_id);

        // Clear existing cache
        {
            let mut file_tree = self.file_tree.write().unwrap();
            file_tree.nodes.clear();
        }

        // Traverse the entire tree to rebuild cache
        let _ = self.get_all_descendants(root_id, max_depth, 0).await?;

        // Save the rebuilt cache
        self.save_cache().await?;

        let file_tree = self.file_tree.read().unwrap();
        println!("Cache refresh complete. Total nodes: {}", file_tree.nodes.len());
        Ok(())
    }

    pub async fn extract_content(&self, id: &str) -> Result<()> {
        self.click_treeitem(id).await?;

        support::sleep(Duration::from_secs(2)).await;

        self.wait_content_load(Duration::from_secs(30)).await?;

        self.driver
            .enter_frame(0)
            .await
            .context("Could not enter main content iFrame!")?;

        let content_collection = self
            .driver
            .find(By::Css(
                "html body.neos-backend div.container div.neos-contentcollection",
            ))
            .await
            .context("Could not find neos-contentcollection!")?;

        let elements = content_collection
            .query(By::Css(":scope > div"))
            .all_from_selector()
            .await?;

        let mut full_text = String::new();
        for element in elements {
            element.scroll_into_view().await?;

            support::sleep(Duration::from_millis(500)).await;

            for field in element.find_all(By::Css("p, ul")).await? {
                let txt = field.text().await?;
                full_text.push_str(&txt);
                full_text.push('\n');
            }
        }

        let raw_path = format!("extracted-{}.txt", id);
        fs::write(&raw_path, &full_text)
            .await
            .context("failed to write tokens")?;

        let tokens: Vec<String> = Lexer::new(&full_text).collect();

        let tok_path = format!("tokens-{}.txt", id);
        let tok_data = tokens.join("\n");
        fs::write(&tok_path, tok_data)
            .await
            .context("failed to write tokens")?;

        self.driver.enter_default_frame().await?;

        Ok(())
    }
}

// DOM CSS selectors and constants used throughout the application
// Centralizing these prevents duplication and makes UI changes easier to handle

use anyhow::Result;
use std::time::Duration;
use thirtyfour::{support, By, WebDriver};
use tokio::time::Instant;

/// CSS selector for folder/treeitem elements by aria-labelledby attribute
pub fn treeitem_selector(id: &str) -> String {
    format!("div[aria-labelledby='{}']", id)
}

/// CSS class name for the tree node header label wrapper
pub const NODE_HEADER_LABEL_WRAPPER: &str = "node__header__labelWrapper___dJ7OH";

/// CSS selector for the tree node toggle/chevron button (with reset class)
pub const NODE_HEADER_CHEVRON_FULL: &str = "a.node__header__chevron___zXVME.reset__reset___2e25U";

/// CSS selector for the tree node toggle/chevron button (without reset class)
pub const NODE_HEADER_CHEVRON: &str = "a.node__header__chevron___zXVME";

/// CSS class name for the node contents container
pub const NODE_CONTENTS: &str = "node__contents___GgwYX";

/// CSS selector for loading indicator
pub const LOADING_INDICATOR: &str = ".style__loadingIndicator__container___1yhsy";

/// CSS selector for dynamic content containers
pub const DYNAMIC_CONTENT_CONTAINER: &str = ".dynamicContent.dynamic-content-container-1";

/// CSS selector for external links in fusion path
pub const FUSION_EXTERNAL_LINKS: &str = "div[data-__neos-fusion-path*='ExternalLinks']";

/// CSS selector for YouTube content in fusion path
pub const FUSION_YOUTUBE: &str = "div[data-__neos-fusion-path*='YouTube']";

/// CSS selector for tutorial articles
pub const FUSION_TUTORIALS: &str = "article[data-__neos-fusion-path*='Plan2net.Schrack4students:Tutorials']";

/// CSS selector for the tree role
pub const TREE_ROLE: &str = "[role='tree']";

/// CSS selector for treeitem role
pub const TREEITEM_ROLE: &str = "[role='treeitem']";

/// Attribute name for aria-labelledby
pub const ATTR_ARIA_LABELLEDBY: &str = "aria-labelledby";

/// Attribute name for aria-expanded
pub const ATTR_ARIA_EXPANDED: &str = "aria-expanded";

/// Attribute name for aria-level
pub const ATTR_ARIA_LEVEL: &str = "aria-level";

/// Attribute name for role
pub const ATTR_ROLE: &str = "role";

/// Wait for page loading indicators to disappear
/// This consolidates the duplicate wait_content_load and wait_for_page_load functions
pub async fn wait_for_loading(driver: &WebDriver, timeout: Duration) -> Result<()> {
    let start = Instant::now();

    loop {
        let loading_bars = driver
            .find_all(By::Css(LOADING_INDICATOR))
            .await?;

        if loading_bars.is_empty() {
            return Ok(());
        }

        if start.elapsed() > timeout {
            return Ok(());
        }

        support::sleep(Duration::from_millis(500)).await;
    }
}

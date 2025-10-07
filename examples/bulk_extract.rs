use anyhow::{Context, Result};
use async_recursion::async_recursion;
use csv::Writer;
use std::{thread::sleep, time::Duration};
use tag_spider_rs::spider::Spider;
use tag_spider_rs::tree::FileTree;
use thirtyfour::{prelude::*, support, By, WebDriver};

static URL: &str = "https://cms.schrackforstudents.com/neos/login";

async fn login(driver: &WebDriver) -> Result<()> {
    // Load credentials from file
    let credentials_content = std::fs::read_to_string("./credentials.json")
        .context("Could not read credentials.json. Please create this file with your username and password.")?;

    #[derive(serde::Deserialize)]
    struct Credentials {
        username: String,
        password: String,
    }

    let creds: Credentials = serde_json::from_str(&credentials_content)
        .context("Invalid JSON in credentials.json")?;

    let credentials = (creds.username, creds.password);

    let username_field = driver
        .find(By::Id("username"))
        .await
        .context("Could not find username field!")?;

    let password_field = driver
        .find(By::Id("password"))
        .await
        .context("Could not find a password field!")?;

    let login_button = driver
        .find(By::ClassName("neos-login-btn"))
        .await
        .context("Could not find login button!")?;

    driver
        .action_chain()
        .click_element(&username_field)
        .send_keys(credentials.0)
        .click_element(&password_field)
        .send_keys(credentials.1)
        .click_element(&login_button)
        .perform()
        .await?;

    support::sleep(Duration::from_secs(2)).await;
    Ok(())
}

async fn find_and_click_folder(driver: &WebDriver, folder_id: &str) -> Result<()> {
    let selector = format!("div[aria-labelledby='{}']", folder_id);
    let folder_element = driver
        .find(By::Css(&selector))
        .await
        .context(format!("Could not find folder with ID: {}", folder_id))?;

    folder_element.scroll_into_view().await?;

    let folder_header = folder_element
        .find(By::ClassName("node__header__labelWrapper___dJ7OH"))
        .await
        .context("Could not find folder header!")?;

    folder_header.click().await?;
    Ok(())
}

async fn expand_folder_if_needed(driver: &WebDriver, folder_id: &str) -> Result<()> {
    let selector = format!("div[aria-labelledby='{}']", folder_id);
    let folder_element = driver.find(By::Css(&selector)).await?;

    let expanded = folder_element.attr("aria-expanded").await?;
    if expanded != Some("true".to_string()) {
        let toggle_button = folder_element
            .find(By::Css(
                "a.node__header__chevron___zXVME.reset__reset___2e25U",
            ))
            .await
            .context("Could not find toggle button!")?;

        toggle_button.click().await?;
        support::sleep(Duration::from_secs(1)).await;
    }
    Ok(())
}

async fn get_folder_children(driver: &WebDriver, folder_id: &str) -> Result<Vec<String>> {
    println!("Getting children for folder: {}", folder_id);

    // First expand the folder to reveal its children
    expand_folder_if_needed(driver, folder_id).await?;
    support::sleep(Duration::from_millis(2000)).await;

    // Now find the specific parent element and look for its children inside the node__contents div
    let parent_selector = format!("div[aria-labelledby='{}']", folder_id);
    let parent_element = driver
        .find(By::Css(&parent_selector))
        .await
        .context("Could not find parent folder element")?;

    println!("Found parent element, now looking for node__contents...");

    // Look for the node__contents div that contains the children (only appears when expanded)
    let contents_divs = parent_element
        .find_all(By::Css("div.node__contents___GgwYX"))
        .await?;

    let mut child_ids = Vec::new();

    for contents_div in contents_divs {
        println!("Found contents div, looking for child treeitems...");

        // Find all direct child treeitems within this contents div
        let child_treeitems = contents_div
            .find_all(By::Css("div[role='treeitem']"))
            .await?;

        println!("Found {} potential child treeitems", child_treeitems.len());

        for child in child_treeitems {
            if let Some(id) = child.attr("aria-labelledby").await? {
                child_ids.push(id.clone());
                println!("Found child: {}", id);
            }
        }
    }

    if child_ids.is_empty() {
        println!("No children found in contents div. Trying fallback method...");

        // Fallback: Use the aria-level approach as before
        let all_items = driver.find_all(By::Css("div[role='treeitem']")).await?;
        let mut found_parent = false;
        let mut parent_level: Option<i32> = None;

        for item in all_items {
            if let Some(id) = item.attr("aria-labelledby").await? {
                if id == folder_id {
                    found_parent = true;
                    if let Some(level_str) = item.attr("aria-level").await? {
                        parent_level = level_str.parse().ok();
                    }
                    continue;
                }

                if found_parent {
                    if let Some(parent_lvl) = parent_level {
                        if let Some(current_level_str) = item.attr("aria-level").await? {
                            if let Ok(current_level) = current_level_str.parse::<i32>() {
                                if current_level == parent_lvl + 1 {
                                    child_ids.push(id.clone());
                                    println!("Found child via fallback: {}", id);
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

    println!("Total children found: {}", child_ids.len());
    Ok(child_ids)
}

#[async_recursion]
async fn get_all_descendants(driver: &WebDriver, folder_id: &str, max_depth: usize, current_depth: usize) -> Result<Vec<String>> {
    let mut all_descendants = Vec::new();

    if current_depth >= max_depth {
        println!("  Reached maximum depth {} for folder: {}", max_depth, folder_id);
        return Ok(all_descendants);
    }

    println!("  Traversing folder at depth {}: {}", current_depth, folder_id);

    // Get direct children of this folder
    let children = get_folder_children(driver, folder_id).await?;

    for child_id in children {
        // Add this child to our list
        all_descendants.push(child_id.clone());
        println!("    Added child: {}", child_id);

        // Try to get children of this child (to see if it's a folder with content)
        // We'll be more permissive here and not fail if a child doesn't have children
        match get_folder_children(driver, &child_id).await {
            Ok(grandchildren) => {
                if !grandchildren.is_empty() {
                    println!("    Child {} has {} grandchildren, recursing...", child_id, grandchildren.len());
                    // This child is also a folder, recurse into it
                    let descendants = get_all_descendants(driver, &child_id, max_depth, current_depth + 1).await?;
                    all_descendants.extend(descendants);
                } else {
                    println!("    Child {} is a leaf node (no children)", child_id);
                }
            },
            Err(_) => {
                // This child might not be a folder or might not be expandable, that's okay
                println!("    Child {} appears to be a leaf node or not expandable", child_id);
            }
        }

        // Small delay between processing children to avoid overwhelming the server
        support::sleep(Duration::from_millis(500)).await;
    }

    println!("  Found {} total descendants for folder: {}", all_descendants.len(), folder_id);
    Ok(all_descendants)
}

async fn extract_breadcrumb_path(driver: &WebDriver) -> Result<String> {
    // Look for breadcrumb elements in the page
    let breadcrumbs = driver
        .find_all(By::Css(".neos-breadcrumb a, .breadcrumb a, [class*='breadcrumb'] a"))
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

    // Fallback: try to extract from page title or URL
    if let Ok(title) = driver.title().await {
        return Ok(title);
    }

    Ok("Unknown Path".to_string())
}

fn extract_youtube_video_id(url: &str) -> Option<String> {
    // Extract video ID from YouTube embed URL like: https://www.youtube.com/embed/H7WzSiZOauA?wmode=transparent&hl=de&rel=0
    if let Some(start) = url.find("/embed/") {
        let after_embed = &url[start + 7..];
        if let Some(end) = after_embed.find('?') {
            Some(after_embed[..end].to_string())
        } else {
            Some(after_embed.to_string())
        }
    } else {
        None
    }
}

async fn validate_url(url: &str) -> String {
    if url.is_empty() {
        return "N/A".to_string();
    }

    println!("    Validating URL: {}", url);

    // Create a client with timeout to avoid hanging
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    match client.head(url).send().await {
        Ok(response) => {
            let status = response.status().as_u16();
            if response.status().is_success() {
                println!("      URL valid ({})", status);
                "Valid".to_string()
            } else if response.status().is_redirection() {
                println!("      URL redirects ({})", status);
                "Redirect".to_string()
            } else {
                println!("      URL error ({})", status);
                format!("Error {}", status)
            }
        }
        Err(e) => {
            println!("      URL validation failed: {}", e);
            "Invalid".to_string()
        }
    }
}

#[derive(Debug)]
struct ContentEntry {
    source_node: String,
    breadcrumb_path: String,
    content_type: String,
    url: String,
    title: String,
    author: String,
    file_type: String,
    size: String,
    url_valid: String,
}

async fn extract_content_from_page(
    driver: &WebDriver,
    node_id: &str,
    _content_types: &[&str],
) -> Result<Vec<ContentEntry>> {
    println!("  Extracting content from treeitem: {}", node_id);

    // Click on the treeitem to navigate to its content
    println!("  Clicking treeitem to load content...");
    find_and_click_folder(driver, node_id).await?;

    println!("  Waiting for page to load after click...");
    support::sleep(Duration::from_secs(5)).await;

    println!("  Looking for dynamic content containers directly in the page...");

    // First, let's check if we need to enter an iframe after all
    let iframes = driver.find_all(By::Tag("iframe")).await?;
    println!("  Found {} iframes on the page", iframes.len());

    if !iframes.is_empty() {
        println!("  Attempting to enter first iframe...");
        match driver.enter_frame(0).await {
            Ok(_) => println!("  Successfully entered iframe"),
            Err(e) => println!("  Failed to enter iframe: {}", e),
        }
    }

    // Look for dynamic content containers
    let dynamic_containers = driver
        .find_all(By::Css(".dynamicContent.dynamic-content-container-1"))
        .await?;

    println!("  Found {} dynamic containers", dynamic_containers.len());

    // If no dynamic containers found, let's see what IS on the page
    if dynamic_containers.is_empty() {
        println!("  No dynamic containers found. Debugging page content...");

        // Check for any elements with 'dynamic' in the class
        if let Ok(dynamic_els) = driver.find_all(By::Css("[class*='dynamic']")).await {
            println!("  Found {} elements with 'dynamic' in class", dynamic_els.len());
            for (i, el) in dynamic_els.iter().enumerate().take(3) {
                if let Ok(class) = el.attr("class").await {
                    println!("    Dynamic element {}: class='{:?}'", i + 1, class);
                }
            }
        }

        // Check for external link elements
        if let Ok(ext_links) = driver.find_all(By::Css("[data-neos-node-type*='ExternalLinks']")).await {
            println!("  Found {} ExternalLinks elements", ext_links.len());
        }

        // Check for any divs with content
        if let Ok(all_divs) = driver.find_all(By::Tag("div")).await {
            println!("  Found {} total div elements on page", all_divs.len());
        }

        // Try alternative selectors that might match
        if let Ok(alt1) = driver.find_all(By::Css(".dynamicContent")).await {
            println!("  Found {} elements with just '.dynamicContent'", alt1.len());
        }

        if let Ok(alt2) = driver.find_all(By::Css("[class*='dynamic-content']")).await {
            println!("  Found {} elements with 'dynamic-content' in class", alt2.len());
        }
    }

    // Extract breadcrumb path
    let breadcrumb_path = extract_breadcrumb_path(driver).await.unwrap_or_else(|_| "Unknown Path".to_string());
    println!("  Breadcrumb path: {}", breadcrumb_path);

    let mut entries = Vec::new();

    for (i, container) in dynamic_containers.iter().enumerate() {
        println!("  Processing dynamic container {} of {}", i + 1, dynamic_containers.len());
        container.scroll_into_view().await?;
        support::sleep(Duration::from_millis(500)).await;

        // The data-neos-node-type is on the paragraph elements, not on a containing div
        // So we need to look for divs that contain ExternalLinks paragraphs
        println!("    Looking for divs containing ExternalLinks paragraphs...");

        // Find divs that have ExternalLinks in their fusion path (these contain the paragraphs)
        let link_container_divs = container
            .find_all(By::Css("div[data-__neos-fusion-path*='ExternalLinks']"))
            .await?;

        println!("    Found {} divs with ExternalLinks in fusion path", link_container_divs.len());

        for (j, item) in link_container_divs.iter().enumerate() {
            println!("    Processing ExternalLinks container div {} of {}", j + 1, link_container_divs.len());
            let mut entry = ContentEntry {
                source_node: node_id.to_string(),
                breadcrumb_path: breadcrumb_path.clone(),
                content_type: "ExternalLink".to_string(),
                url: String::new(),
                title: String::new(),
                author: String::new(),
                file_type: String::new(),
                size: String::new(),
                url_valid: String::new(),
            };

            // Extract URL
            println!("      Looking for URL...");
            if let Ok(url_element) = item.query(By::Css("p[property='typo3:url']")).first().await {
                if let Ok(url) = url_element.text().await {
                    entry.url = url.trim().to_string();
                    println!("      Found URL: {}", entry.url);
                }
            } else {
                println!("      No URL element found");
            }

            // Extract Title
            println!("      Looking for Title...");
            if let Ok(title_element) = item
                .query(By::Css("p[property='typo3:title']"))
                .first()
                .await
            {
                if let Ok(title) = title_element.text().await {
                    entry.title = title.trim().to_string();
                    println!("      Found Title: {}", entry.title);
                }
            } else {
                println!("      No Title element found");
            }

            // Extract Author
            println!("      Looking for Author...");
            if let Ok(author_element) = item
                .query(By::Css("p[property='typo3:author']"))
                .first()
                .await
            {
                if let Ok(author) = author_element.text().await {
                    entry.author = author.trim().to_string();
                    println!("      Found Author: {}", entry.author);
                }
            } else {
                println!("      No Author element found");
            }

            // Extract Type
            println!("      Looking for Type...");
            if let Ok(type_element) = item
                .query(By::Css("p[property='typo3:type']"))
                .first()
                .await
            {
                if let Ok(file_type) = type_element.text().await {
                    entry.file_type = file_type.trim().to_string();
                    println!("      Found Type: {}", entry.file_type);
                }
            } else {
                println!("      No Type element found");
            }

            // Extract Size
            println!("      Looking for Size...");
            if let Ok(size_element) = item
                .query(By::Css("p[property='typo3:size']"))
                .first()
                .await
            {
                if let Ok(size) = size_element.text().await {
                    entry.size = size.trim().to_string();
                    println!("      Found Size: {}", entry.size);
                }
            } else {
                println!("      No Size element found");
            }

            // Validate URL if present
            entry.url_valid = validate_url(&entry.url).await;

            // Only add entry if we have at least a URL or title
            if !entry.url.is_empty() || !entry.title.is_empty() {
                entries.push(entry);
            }
        }

        // Now look for YouTube content in the same containers
        println!("    Looking for YouTube content...");
        let youtube_container_divs = container
            .find_all(By::Css("div[data-__neos-fusion-path*='YouTube']"))
            .await?;

        println!("    Found {} divs with YouTube in fusion path", youtube_container_divs.len());

        for (j, item) in youtube_container_divs.iter().enumerate() {
            println!("    Processing YouTube container div {} of {}", j + 1, youtube_container_divs.len());
            let mut entry = ContentEntry {
                source_node: node_id.to_string(),
                breadcrumb_path: breadcrumb_path.clone(),
                content_type: "YouTube".to_string(),
                url: String::new(),
                title: String::new(),
                author: String::new(),
                file_type: "video".to_string(),
                size: String::new(),
                url_valid: String::new(),
            };

            // Extract YouTube URL from iframe src
            println!("      Looking for YouTube iframe...");
            if let Ok(iframe_element) = item.query(By::Css("iframe")).first().await {
                if let Ok(src_url) = iframe_element.attr("src").await {
                    if let Some(url) = src_url {
                        entry.url = url.trim().to_string();
                        println!("      Found YouTube URL: {}", entry.url);

                        // Extract video ID from YouTube URL for title
                        if let Some(video_id) = extract_youtube_video_id(&entry.url) {
                            entry.title = format!("YouTube Video ({})", video_id);
                        }
                    }
                }
            } else {
                println!("      No YouTube iframe found");
            }

            // Validate URL if present
            entry.url_valid = validate_url(&entry.url).await;

            // Only add entry if we have a URL
            if !entry.url.is_empty() {
                entries.push(entry);
            }
        }
    }

    // Exit iframe if we entered one
    if !iframes.is_empty() {
        let _ = driver.enter_default_frame().await;
    }

    println!("  Extracted {} entries from {}", entries.len(), node_id);
    Ok(entries)
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    // Configuration - modify these as needed
    let target_folder_id = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "treeitem-c6643bf0-label".to_string()); // Default folder ID
    let content_types = &["p", "h1", "h2", "h3", "ul", "ol"]; // Content types to extract
    let output_file = "extracted_links.csv";

    println!("Starting bulk extraction from folder: {}", target_folder_id);
    println!("Looking for external link content");

    // Create a minimal FileTree just to satisfy Spider constructor
    let dummy_tree = FileTree::default();
    let spider = Spider::new(DesiredCapabilities::firefox(), URL, dummy_tree).await?;

    login(&spider.driver).await?;

    sleep(Duration::from_secs(10));

    // Navigate to the target folder and expand it
    expand_folder_if_needed(&spider.driver, &target_folder_id).await?;

    // Get all descendants (children, grandchildren, etc.) of the target folder
    let max_traversal_depth = 5; // Prevent infinite recursion, adjust as needed
    println!("Starting recursive traversal with max depth: {}", max_traversal_depth);
    let child_ids = get_all_descendants(&spider.driver, &target_folder_id, max_traversal_depth, 0).await?;
    println!("Found {} total items to process (including all descendants)", child_ids.len());

    // Create CSV writer
    let mut csv_writer = Writer::from_path(output_file).context("Failed to create CSV file")?;

    // Write CSV header
    csv_writer
        .write_record(&["Source Node", "Breadcrumb Path", "Content Type", "URL", "Title", "Author", "File Type", "Size", "URL Valid"])
        .context("Failed to write CSV header")?;

    let mut all_entries = Vec::new();
    let mut successful = 0;
    let mut failed = 0;

    for (index, child_id) in child_ids.iter().enumerate() {
        println!(
            "\n=== Processing item {} of {} (ID: {}) ===",
            index + 1,
            child_ids.len(),
            child_id
        );

        // Since descendants are treeitems, extract content directly from each one
        match extract_content_from_page(&spider.driver, child_id, content_types).await {
            Ok(entries) => {
                if !entries.is_empty() {
                    println!("✓ Found {} entries in item {}", entries.len(), child_id);
                    all_entries.extend(entries);
                    successful += 1;
                } else {
                    println!("⚠ No content found in item {}", child_id);
                }
            }
            Err(e) => {
                eprintln!("✗ Failed to extract from item {}: {}", child_id, e);
                failed += 1;
            }
        }

        support::sleep(Duration::from_millis(1500)).await;
    }

    // Also extract from the target folder itself
    println!("Processing target folder: {}", target_folder_id);
    match extract_content_from_page(&spider.driver, &target_folder_id, content_types).await {
        Ok(entries) => {
            if !entries.is_empty() {
                println!("Found {} entries in target folder", entries.len());
                all_entries.extend(entries);
                successful += 1;
            }
        }
        Err(e) => {
            eprintln!(
                "Failed to extract from target folder {}: {}",
                target_folder_id, e
            );
            failed += 1;
        }
    }

    // Write all entries to CSV
    for entry in &all_entries {
        csv_writer
            .write_record(&[
                &entry.source_node,
                &entry.breadcrumb_path,
                &entry.content_type,
                &entry.url,
                &entry.title,
                &entry.author,
                &entry.file_type,
                &entry.size,
                &entry.url_valid,
            ])
            .context("Failed to write CSV record")?;
    }

    csv_writer.flush().context("Failed to flush CSV writer")?;

    println!("\nBulk extraction complete!");
    println!("Total entries found: {}", all_entries.len());
    println!("Successfully processed pages: {}", successful);
    println!("Failed pages: {}", failed);
    println!("CSV output saved to: {}", output_file);

    spider.driver.quit().await?;
    Ok(())
}

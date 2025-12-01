// src/main.rs
use anyhow::{Context, Result};
use crossterm::event::{Event, KeyCode};
use csv::{Reader, Writer};
use futures::stream::{self, StreamExt};
use std::io::{self, Write};
use std::path::PathBuf;
use std::{collections::HashMap, fs, time::Duration};
use tag_spider_rs::auth::{login, is_relogin_dialog_present, handle_relogin_dialog};
use tag_spider_rs::content::ContentEntry;
use tag_spider_rs::dom_constants::*;
use tag_spider_rs::spider::Spider;
use tag_spider_rs::tree::FileTree;
use thirtyfour::{prelude::*, support, By, WebDriver, WebElement};

static URL: &str = "https://cms.schrackforstudents.com/neos/login";
static TAGPATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/resources/tags.csv");

fn extract_youtube_video_id(url: &str) -> Option<String> {
    // Extract video ID from YouTube embed URL like: https://www.youtube.com/embed/H7WzSiZOauA?wmode=transparent&hl=de&rel=0
    if let Some(start) = url.find("/embed/") {
        let after_embed = &url[start + 7..];
        if let Some(end) = after_embed.find('?') {
            Some(after_embed[..end].to_string())
        } else {
            Some(after_embed.to_string())
        }
    }
    // Extract video ID from YouTube watch URL like: https://www.youtube.com/watch?v=HGk7PYSJEsM
    else if let Some(start) = url.find("watch?v=") {
        let after_v = &url[start + 8..];
        if let Some(end) = after_v.find('&') {
            Some(after_v[..end].to_string())
        } else {
            Some(after_v.to_string())
        }
    } else {
        None
    }
}

async fn extract_external_links(
    container: &WebElement,
    node_id: &str,
    breadcrumb_path: &str,
) -> Result<Vec<ContentEntry>> {
    let mut entries = Vec::new();

    println!("    Looking for divs containing ExternalLinks paragraphs...");
    let link_container_divs = container
        .find_all(By::Css(FUSION_EXTERNAL_LINKS))
        .await?;

    println!(
        "    Found {} divs with ExternalLinks in fusion path",
        link_container_divs.len()
    );

    for (j, item) in link_container_divs.iter().enumerate() {
        println!(
            "    Processing ExternalLinks container div {} of {}",
            j + 1,
            link_container_divs.len()
        );
        let mut entry = ContentEntry {
            source_node: node_id.to_string(),
            breadcrumb_path: breadcrumb_path.to_string(),
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

        // URL validation will happen later in batch
        entry.url_valid = String::new();

        // Only add entry if we have at least a URL or title
        if !entry.url.is_empty() || !entry.title.is_empty() {
            entries.push(entry);
        }
    }

    Ok(entries)
}

async fn extract_youtube_content(
    container: &WebElement,
    node_id: &str,
    breadcrumb_path: &str,
) -> Result<Vec<ContentEntry>> {
    let mut entries = Vec::new();

    println!("    Looking for YouTube content...");
    let youtube_container_divs = container
        .find_all(By::Css(FUSION_YOUTUBE))
        .await?;

    println!(
        "    Found {} divs with YouTube in fusion path",
        youtube_container_divs.len()
    );

    for (j, item) in youtube_container_divs.iter().enumerate() {
        println!(
            "    Processing YouTube container div {} of {}",
            j + 1,
            youtube_container_divs.len()
        );
        let mut entry = ContentEntry {
            source_node: node_id.to_string(),
            breadcrumb_path: breadcrumb_path.to_string(),
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
            if let Ok(Some(url)) = iframe_element.attr("src").await {
                entry.url = url.trim().to_string();
                println!("      Found YouTube URL: {}", entry.url);

                // Extract video ID from YouTube URL for title
                if let Some(video_id) = extract_youtube_video_id(&entry.url) {
                    entry.title = format!("YouTube Video ({})", video_id);
                }
            }
        } else {
            println!("      No YouTube iframe found");
        }

        // URL validation will happen later in batch
        entry.url_valid = String::new();

        // Only add entry if we have a URL
        if !entry.url.is_empty() {
            entries.push(entry);
        }
    }

    Ok(entries)
}

async fn extract_tutorial_content(
    driver: &WebDriver,
    node_id: &str,
    breadcrumb_path: &str,
) -> Result<Vec<ContentEntry>> {
    let mut entries = Vec::new();

    println!("    Looking for Tutorial content...");
    let tutorial_articles = driver
        .find_all(By::Css(FUSION_TUTORIALS))
        .await?;

    println!("    Found {} tutorial articles", tutorial_articles.len());

    for (i, article) in tutorial_articles.iter().enumerate() {
        println!(
            "    Processing Tutorial article {} of {}",
            i + 1,
            tutorial_articles.len()
        );
        let mut entry = ContentEntry {
            source_node: node_id.to_string(),
            breadcrumb_path: breadcrumb_path.to_string(),
            content_type: "Tutorial".to_string(),
            url: String::new(),
            title: String::new(),
            author: String::new(),
            file_type: "video".to_string(),
            size: String::new(),
            url_valid: String::new(),
        };

        // Extract YouTube URL from div[property='typo3:videoUrl']
        println!("      Looking for video URL...");
        if let Ok(url_div) = article
            .query(By::Css("div[property='typo3:videoUrl']"))
            .first()
            .await
        {
            // First try to get text directly from the div
            if let Ok(url_text) = url_div.text().await {
                let url = url_text.trim().to_string();
                if !url.is_empty() {
                    entry.url = url.clone();
                    println!("      Found URL: {}", entry.url);

                    // Extract video ID from YouTube URL for title
                    if let Some(video_id) = extract_youtube_video_id(&entry.url) {
                        entry.title = format!("YouTube Tutorial ({})", video_id);
                    }
                }
            }
        } else {
            println!("      No video URL element found");
        }

        // URL validation will happen later in batch
        if !entry.url.is_empty() {
            entry.url_valid = String::new();
            entries.push(entry);
        }
    }

    Ok(entries)
}

async fn validate_url(url: &str) -> String {
    if url.is_empty() {
        return "N/A".to_string();
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    match client.head(url).send().await {
        Ok(response) => {
            let status = response.status().as_u16();
            if response.status().is_success() {
                "Valid".to_string()
            } else if response.status().is_redirection() {
                "Redirect".to_string()
            } else {
                format!("Error {status}")
            }
        }
        Err(_) => {
            "Invalid".to_string()
        }
    }
}

/// Validate URLs concurrently in batches
async fn validate_urls_concurrent(entries: &mut [ContentEntry], concurrency: usize) {
    let total_urls = entries.len();
    println!("Validating {} URLs with concurrency {}...", total_urls, concurrency);

    let urls_with_indices: Vec<(usize, String)> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| (i, e.url.clone()))
        .collect();

    let validated: Vec<(usize, String)> = stream::iter(urls_with_indices)
        .map(|(idx, url)| async move {
            let result = validate_url(&url).await;
            if (idx + 1) % 10 == 0 {
                println!("  Validated {}/{} URLs", idx + 1, total_urls);
            }
            (idx, result)
        })
        .buffer_unordered(concurrency)
        .collect()
        .await;

    // Update entries with validation results
    for (idx, validation_result) in validated {
        entries[idx].url_valid = validation_result;
    }

    println!("URL validation complete!");
}


async fn extract_content_from_page(
    spider: &Spider,
    node_id: &str,
) -> Result<Vec<ContentEntry>> {
    println!("  Extracting content from treeitem: {node_id}");

    // Use Spider's retry-wrapped click method
    spider.retry_with_relogin(|| async {
        spider.click_treeitem(node_id).await
    }, 3).await?;

    println!("  Waiting for page to load...");
    wait_for_loading(&spider.driver, Duration::from_secs(30)).await?;

    // Use Spider's content context finder
    let in_iframe = spider.find_content_context().await?;

    // Use Spider's breadcrumb extractor
    let breadcrumb_path = spider.extract_breadcrumb_path()
        .await
        .unwrap_or_else(|_| "Unknown Path".to_string());
    println!("  Breadcrumb path: {breadcrumb_path}");

    let mut entries = Vec::new();

    // Look for dynamic content containers
    let dynamic_containers = spider.driver
        .find_all(By::Css(DYNAMIC_CONTENT_CONTAINER))
        .await?;

    println!("  Found {} dynamic containers", dynamic_containers.len());

    // Extract content from dynamic containers
    for (i, container) in dynamic_containers.iter().enumerate() {
        println!(
            "  Processing dynamic container {} of {}",
            i + 1,
            dynamic_containers.len()
        );
        container.scroll_into_view().await?;
        // Reduced from 500ms to 300ms
        support::sleep(Duration::from_millis(300)).await;

        // Extract ExternalLinks - but mark URLs as Pending for batch validation
        let mut external_links = extract_external_links(container, node_id, &breadcrumb_path).await?;
        for entry in &mut external_links {
            entry.url_valid = "Pending".to_string();
        }
        entries.extend(external_links);

        // Extract YouTube content - but mark URLs as Pending for batch validation
        let mut youtube_content = extract_youtube_content(container, node_id, &breadcrumb_path).await?;
        for entry in &mut youtube_content {
            entry.url_valid = "Pending".to_string();
        }
        entries.extend(youtube_content);
    }

    // Extract Tutorial content (not in dynamic containers) - mark URLs as Pending for batch validation
    let mut tutorial_content = extract_tutorial_content(&spider.driver, node_id, &breadcrumb_path).await?;
    for entry in &mut tutorial_content {
        entry.url_valid = "Pending".to_string();
    }
    entries.extend(tutorial_content);

    // Exit iframe if we entered one
    if in_iframe {
        let _ = spider.driver.enter_default_frame().await;
    }

    println!("  Extracted {} entries from {}", entries.len(), node_id);
    Ok(entries)
}

/// Load CSV data for tags.
fn load_csv_data(path: &str) -> Result<HashMap<String, String>> {
    let mut tags: HashMap<String, String> = HashMap::new();
    let mut reader = Reader::from_path(path)?;

    for line in reader.records() {
        let record = line?;
        tags.insert(record[0].to_string(), record[1].to_string());
    }

    Ok(tags)
}

/// Example function to add tags.
async fn add_tags(clear: bool, spider: &Spider) -> Result<()> {
    let tags = load_csv_data(TAGPATH).unwrap();
    let iframe = spider.driver
        .query(By::Css(r#"iframe[name="neos-content-main"]"#))
        .first()
        .await?;
    iframe.clone().enter_frame().await?;

    let content_collection = spider.driver
        .query(By::Css(
            "html body.neos-backend div.container div.neos-contentcollection",
        ))
        .first()
        .await?;
    let questions = content_collection
        .find_all(By::Css("p.neos-inline-editable.questionTitle"))
        .await?;

    for question in questions {
        question.scroll_into_view().await?;
        let text = question.text().await?;
        let id = text.split(' ').next().unwrap();
        let value = tags.get(id).unwrap();

        question.click().await?;
        spider.driver.enter_default_frame().await?;

        let tag_textbox = spider.driver
            .query(By::Css("#__neos__editor__property---Tags"))
            .first()
            .await?;

        spider.driver
            .action_chain()
            .click_element(&tag_textbox)
            .key_down(thirtyfour::Key::Control)
            .send_keys("a")
            .key_up(thirtyfour::Key::Control)
            .send_keys(thirtyfour::Key::Backspace)
            .perform()
            .await?;

        if !clear {
            if let Some(val) = tags.get(id) {
                tag_textbox.send_keys(val).await?;
            } else {
                eprintln!("Error: key {id} not found! Skipping...");
                iframe.clone().enter_frame().await?;
                continue;
            }
        }

        let apply_button = spider.driver
            .query(By::Css("#neos-Inspector-Apply"))
            .first()
            .await?;
        apply_button.click().await?;

        println!("{id} -> {value}");
        iframe.clone().enter_frame().await?;
        support::sleep(Duration::new(1, 0)).await;
    }
    spider.driver.enter_default_frame().await?;
    Ok(())
}

fn read_line() -> String {
    let mut input = String::new();
    print!("> ");
    io::stdout().flush().unwrap();
    io::stdin().read_line(&mut input).unwrap();
    input.trim().to_string()
}

fn ask_yes_no(question: &str) -> bool {
    loop {
        println!("{question} (y/n)");
        let input = read_line().to_lowercase();
        match input.as_str() {
            "y" | "yes" => return true,
            "n" | "no" => return false,
            _ => println!("Please enter 'y' or 'n'"),
        }
    }
}

async fn bulk_extract_content(spider: &Spider) -> Result<()> {
    println!("\n=== Bulk Content Extraction ===");

    println!("Enter the treeitem ID to start extraction from:");
    let target_folder_id = read_line();

    if target_folder_id.is_empty() {
        println!("No folder ID provided. Using default: treeitem-c6643bf0-label");
        let target_folder_id = "treeitem-c6643bf0-label";
        return do_bulk_extract(spider, target_folder_id).await;
    }

    do_bulk_extract(spider, &target_folder_id).await
}

async fn do_bulk_extract(spider: &Spider, target_folder_id: &str) -> Result<()> {
    let validate_urls = ask_yes_no("Do you want to validate URLs? (This will be done concurrently at the end)");

    println!("Starting bulk extraction from folder: {target_folder_id}");

    println!("Checking if target folder exists on current page...");

    // Check if we're on the right page and logged in
    let page_title = spider.driver
        .title()
        .await
        .unwrap_or_else(|_| "Unknown".to_string());
    println!("Current page title: {}", page_title);

    // Wait a bit to ensure page is fully loaded
    println!("Waiting for page to load completely...");
    support::sleep(Duration::from_secs(2)).await;

    // Navigate to the target folder and expand it
    spider.expand_treeitem_if_needed(target_folder_id).await?;

    // Get all descendants (children, grandchildren, etc.) of the target folder
    let max_traversal_depth = 5;
    println!("Starting recursive traversal with max depth: {max_traversal_depth}");
    let child_ids = spider.get_all_descendants(target_folder_id, max_traversal_depth, 0).await?;
    println!(
        "Found {} total items to process (including all descendants)",
        child_ids.len()
    );

    // Create embedded_content directory if it doesn't exist
    fs::create_dir_all("./embedded_content")
        .context("Failed to create embedded_content directory")?;

    // Create CSV writer with entry ID as filename
    let output_file = format!("./embedded_content/{target_folder_id}.csv");
    println!("CSV will be saved to: {output_file}");
    let mut csv_writer = Writer::from_path(&output_file).context("Failed to create CSV file")?;

    // Write CSV header
    csv_writer
        .write_record([
            "Source Node",
            "Breadcrumb Path",
            "Content Type",
            "URL",
            "Title",
            "Author",
            "File Type",
            "Size",
            "URL Valid",
        ])
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

        // Check for relogin dialog before processing each item
        if is_relogin_dialog_present(&spider.driver).await {
            println!("Relogin dialog detected before processing item {child_id}");
            match handle_relogin_dialog(&spider.driver).await {
                Ok(true) => println!("Relogin successful, continuing..."),
                Ok(false) => println!("Relogin failed, but continuing..."),
                Err(e) => println!("Error handling relogin: {e}, continuing..."),
            }
        }

        match extract_content_from_page(spider, child_id).await {
            Ok(entries) => {
                if !entries.is_empty() {
                    println!("✓ Found {} entries in item {}", entries.len(), child_id);
                    all_entries.extend(entries);
                    successful += 1;
                } else {
                    println!("⚠ No content found in item {child_id}");
                }
            }
            Err(e) => {
                eprintln!("✗ Failed to extract from item {child_id}: {e}");
                failed += 1;
            }
        }

        // Reduced sleep delay from 1500ms to 800ms
        support::sleep(Duration::from_millis(800)).await;
    }

    // Also extract from the target folder itself
    println!("\nProcessing target folder: {target_folder_id}");
    match extract_content_from_page(spider, target_folder_id).await {
        Ok(entries) => {
            if !entries.is_empty() {
                println!("Found {} entries in target folder", entries.len());
                all_entries.extend(entries);
                successful += 1;
            }
        }
        Err(e) => {
            eprintln!("Failed to extract from target folder {target_folder_id}: {e}");
            failed += 1;
        }
    }

    println!("\n=== Content extraction complete! ===");
    println!("Total entries found: {}", all_entries.len());
    println!("Successfully processed pages: {successful}");
    println!("Failed pages: {failed}");

    // Validate URLs concurrently if requested
    if validate_urls && !all_entries.is_empty() {
        println!("\n=== Starting concurrent URL validation ===");
        validate_urls_concurrent(&mut all_entries, 15).await;
    } else if !validate_urls {
        // Mark all as skipped
        for entry in &mut all_entries {
            entry.url_valid = "Skipped".to_string();
        }
        println!("URL validation skipped by user");
    }

    // Write all entries to CSV
    println!("\nWriting results to CSV...");
    for entry in &all_entries {
        csv_writer
            .write_record([
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

    println!("\n=== Bulk extraction complete! ===");
    println!("CSV output saved to: {output_file}");

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let filetree = FileTree::from_json_file(PathBuf::from("resources/tree.json"))
        .context("Could not create filetree from json")?;

    // Check for headless mode via environment variable
    let headless = std::env::var("HEADLESS")
        .unwrap_or_else(|_| "false".to_string())
        .to_lowercase()
        == "true";

    let caps = if headless {
        println!("Running in headless mode");
        let mut caps = DesiredCapabilities::firefox();
        caps.set_headless()?;
        caps
    } else {
        println!("Running in normal (visible) mode. Set HEADLESS=true environment variable to run headless.");
        DesiredCapabilities::firefox()
    };

    let spider = Spider::new(caps, URL, filetree).await?;

    // Log in.
    login(&spider.driver).await?;

    if !headless {
        println!("Login attempted. Please manually navigate to the CMS and log in if needed.");
    }
    println!("Waiting 10 seconds for you to complete login and navigation...");
    support::sleep(Duration::from_secs(10)).await;

    let welcome_message = r#"
    Welcome to the tag spider. You can do the following actions by pressing:

    q -> quit the program
    a -> add tags (must be in question answer environment)
    c -> clear tags (must be in question answer environment)
    d -> bulk extract and validate dynamic content from folders
    "#;

    loop {
        println!("{welcome_message}");
        if let Event::Key(event) = crossterm::event::read().unwrap() {
            match event.code {
                KeyCode::Char('q') => break,
                KeyCode::Char('a') => add_tags(false, &spider).await?,
                KeyCode::Char('c') => add_tags(true, &spider).await?,
                KeyCode::Char('d') => {
                    bulk_extract_content(&spider).await?;
                }
                _ => {}
            }
        }
    }

    spider.driver.quit().await?;
    Ok(())
}

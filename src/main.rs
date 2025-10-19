// src/main.rs
use anyhow::{Context, Result};
use async_recursion::async_recursion;
use crossterm::event::{Event, KeyCode};
use csv::{Reader, Writer};
use std::io::{self, Write};
use std::path::PathBuf;
use std::{collections::HashMap, fs, time::Duration};
use tag_spider_rs::spider::Spider;
use tag_spider_rs::tree::FileTree;
use thirtyfour::{prelude::*, support, By, WebDriver};

static URL: &str = "https://cms.schrackforstudents.com/neos/login";
static TAGPATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/resources/tags.csv");

#[derive(serde::Deserialize)]
struct Credentials {
    username: String,
    password: String,
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

/// Check if relogin dialog is present
async fn is_relogin_dialog_present(driver: &WebDriver) -> bool {
    driver
        .find(By::Id("neos-ReloginDialog"))
        .await
        .is_ok()
}

/// Handle relogin dialog if present
async fn handle_relogin_dialog(driver: &WebDriver) -> Result<bool> {
    if !is_relogin_dialog_present(driver).await {
        return Ok(false);
    }

    println!("Relogin dialog detected! Attempting to login again...");

    // Get credentials
    let credentials = get_credentials()?;

    // Find username field in relogin dialog
    let username_field = driver
        .find(By::Name("__authentication[Neos][Flow][Security][Authentication][Token][UsernamePassword][username]"))
        .await
        .context("Could not find username field in relogin dialog!")?;

    // Find password field in relogin dialog
    let password_field = driver
        .find(By::Name("__authentication[Neos][Flow][Security][Authentication][Token][UsernamePassword][password]"))
        .await
        .context("Could not find password field in relogin dialog!")?;

    // Find login button in relogin dialog
    let login_button = driver
        .find(By::Css("button.style__btn___3rhzP.style__btn--brand___1ZsvX.style__loginButton___1nLYF"))
        .await
        .context("Could not find login button in relogin dialog!")?;

    // Clear existing values and enter credentials
    username_field.clear().await?;
    username_field.send_keys(&credentials.0).await?;

    password_field.clear().await?;
    password_field.send_keys(&credentials.1).await?;

    // Click login button
    login_button.click().await?;

    // Wait for login to complete
    support::sleep(Duration::from_secs(3)).await;

    // Check if dialog is gone
    let login_successful = !is_relogin_dialog_present(driver).await;

    if login_successful {
        println!("Relogin successful!");
    } else {
        println!("Relogin may have failed - dialog still present");
    }

    Ok(login_successful)
}

/// Get credentials from files
fn get_credentials() -> Result<(String, String)> {
    let credential_paths = [
        PathBuf::from("/run/secrets/cms-pswd"),
        PathBuf::from("./credentials.json"),
        PathBuf::from("./config/credentials.json"),
    ];

    for path in &credential_paths {
        match fs::read_to_string(path) {
            Ok(content) => {
                let creds: Credentials = serde_json::from_str(&content).context(
                    "Credentials are not valid JSON with fields 'password' and 'username'",
                )?;
                return Ok((creds.username, creds.password));
            }
            Err(_) => continue,
        }
    }

    Err(anyhow::anyhow!("No credentials file found"))
}

/// Log in using the provided WebDriver.
pub async fn login(driver: &WebDriver) -> Result<()> {
    let credentials = get_credentials()?;

    // Find the login elements
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

    // Perform the login action
    driver
        .action_chain()
        .click_element(&username_field)
        .send_keys(&credentials.0)
        .click_element(&password_field)
        .send_keys(&credentials.1)
        .click_element(&login_button)
        .perform()
        .await?;

    support::sleep(Duration::from_secs(2)).await;

    Ok(())
}

/// Retry wrapper that handles relogin dialogs automatically
async fn retry_with_relogin<F, Fut, T>(
    driver: &WebDriver,
    operation: F,
    max_retries: usize,
) -> Result<T>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<T>>,
{
    let mut last_error = None;

    for attempt in 0..=max_retries {
        // Check for relogin dialog before attempting operation
        if is_relogin_dialog_present(driver).await {
            println!("Relogin dialog detected before operation attempt {}", attempt + 1);
            match handle_relogin_dialog(driver).await {
                Ok(true) => {
                    println!("Relogin successful, continuing with operation...");
                    // Give some time for the page to settle after relogin
                    support::sleep(Duration::from_secs(2)).await;
                },
                Ok(false) => {
                    return Err(anyhow::anyhow!("Relogin dialog present but login failed"));
                },
                Err(e) => {
                    return Err(anyhow::anyhow!("Failed to handle relogin dialog: {}", e));
                }
            }
        }

        // Attempt the operation
        match operation().await {
            Ok(result) => return Ok(result),
            Err(e) => {
                let error_msg = e.to_string();

                // Check if the error is due to relogin dialog intercepting clicks
                if error_msg.contains("neos-ReloginDialog") ||
                   error_msg.contains("element click intercepted") ||
                   error_msg.contains("ElementClickInterceptedError") {

                    println!("Operation failed due to relogin dialog interference (attempt {})", attempt + 1);

                    if attempt < max_retries {
                        // Try to handle relogin dialog
                        match handle_relogin_dialog(driver).await {
                            Ok(true) => {
                                println!("Relogin successful, retrying operation...");
                                support::sleep(Duration::from_secs(2)).await;
                                continue;
                            },
                            Ok(false) => {
                                println!("Relogin failed, but will retry operation anyway");
                                support::sleep(Duration::from_secs(1)).await;
                                continue;
                            },
                            Err(relogin_err) => {
                                println!("Failed to handle relogin: {}", relogin_err);
                                support::sleep(Duration::from_secs(1)).await;
                                continue;
                            }
                        }
                    }
                }

                last_error = Some(e);

                if attempt < max_retries {
                    println!("Operation failed (attempt {}), retrying in 2 seconds...", attempt + 1);
                    support::sleep(Duration::from_secs(2)).await;
                }
            }
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("All retry attempts failed")))
}

/// Helper function for common operations that need relogin protection
async fn safe_click_element(driver: &WebDriver, element: &thirtyfour::WebElement) -> Result<()> {
    retry_with_relogin(driver, || async {
        element.click().await.map_err(|e| anyhow::anyhow!("Click failed: {}", e))
    }, 3).await
}

async fn find_and_click_folder(driver: &WebDriver, folder_id: &str) -> Result<()> {
    let selector = format!("div[aria-labelledby='{folder_id}']");
    let folder_element = driver
        .find(By::Css(&selector))
        .await
        .context(format!("Could not find folder with ID: {folder_id}"))?;

    folder_element.scroll_into_view().await?;

    let folder_header = folder_element
        .find(By::ClassName("node__header__labelWrapper___dJ7OH"))
        .await
        .context("Could not find folder header!")?;

    folder_header.click().await?;
    Ok(())
}

async fn expand_folder_if_needed(driver: &WebDriver, folder_id: &str) -> Result<()> {
    retry_with_relogin(driver, || async {
        let selector = format!("div[aria-labelledby='{folder_id}']");
        let folder_element = driver.find(By::Css(&selector)).await.context(format!(
            "Could not find folder element '{folder_id}'. Make sure you're on the correct page and logged in."))?;

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
    }, 3).await
}

async fn is_folder_expandable(driver: &WebDriver, folder_id: &str) -> Result<bool> {
    let selector = format!("div[aria-labelledby='{folder_id}']");
    let folder_element = driver.find(By::Css(&selector)).await.context(format!(
        "Could not find folder element '{folder_id}'"))?;

    // Check if the folder has a chevron button (indicates it's expandable)
    let chevron_exists = folder_element
        .find(By::Css("a.node__header__chevron___zXVME"))
        .await
        .is_ok();

    Ok(chevron_exists)
}

async fn get_folder_children(driver: &WebDriver, folder_id: &str) -> Result<Vec<String>> {
    println!("Getting children for folder: {folder_id}");

    // First check if the folder is expandable
    if !is_folder_expandable(driver, folder_id).await? {
        println!("Folder {folder_id} is not expandable (no chevron found)");
        return Ok(Vec::new());
    }

    expand_folder_if_needed(driver, folder_id).await?;
    support::sleep(Duration::from_millis(2000)).await;

    let parent_selector = format!("div[aria-labelledby='{folder_id}']");
    let parent_element = driver
        .find(By::Css(&parent_selector))
        .await
        .context("Could not find parent folder element")?;

    println!("Found parent element, now looking for node__contents...");

    let contents_divs = parent_element
        .find_all(By::Css("div.node__contents___GgwYX"))
        .await?;

    let mut child_ids = Vec::new();

    for contents_div in contents_divs {
        println!("Found contents div, looking for child treeitems...");

        let child_treeitems = contents_div
            .find_all(By::Css("div[role='treeitem']"))
            .await?;

        println!("Found {} potential child treeitems", child_treeitems.len());

        for child in child_treeitems {
            if let Some(id) = child.attr("aria-labelledby").await? {
                child_ids.push(id.clone());
                println!("Found child: {id}");
            }
        }
    }

    if child_ids.is_empty() {
        println!("No children found in contents div. Trying fallback method...");

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
                                    println!("Found child via fallback: {id}");
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
async fn get_all_descendants(
    driver: &WebDriver,
    folder_id: &str,
    max_depth: usize,
    current_depth: usize,
) -> Result<Vec<String>> {
    let mut all_descendants = Vec::new();

    if current_depth >= max_depth {
        println!("  Reached maximum depth {max_depth} for folder: {folder_id}");
        return Ok(all_descendants);
    }

    println!("  Traversing folder at depth {current_depth}: {folder_id}");

    let children = get_folder_children(driver, folder_id).await?;

    for child_id in children {
        all_descendants.push(child_id.clone());
        println!("    Added child: {child_id}");

        // Check if child is expandable before trying to get its children
        match is_folder_expandable(driver, &child_id).await {
            Ok(true) => {
                // Child is expandable, get its children
                match get_folder_children(driver, &child_id).await {
                    Ok(grandchildren) => {
                        if !grandchildren.is_empty() {
                            println!(
                                "    Child {} has {} grandchildren, recursing...",
                                child_id,
                                grandchildren.len()
                            );
                            let descendants =
                                get_all_descendants(driver, &child_id, max_depth, current_depth + 1)
                                    .await?;
                            all_descendants.extend(descendants);
                        } else {
                            println!("    Child {child_id} is expandable but has no children");
                        }
                    }
                    Err(e) => {
                        println!("    Failed to get children for {child_id}: {e}");
                    }
                }
            }
            Ok(false) => {
                println!("    Child {child_id} is a leaf node (no chevron indicator)");
            }
            Err(e) => {
                println!("    Could not check if {child_id} is expandable: {e}");
            }
        }

        support::sleep(Duration::from_millis(500)).await;
    }

    println!(
        "  Found {} total descendants for folder: {}",
        all_descendants.len(),
        folder_id
    );
    Ok(all_descendants)
}

async fn extract_breadcrumb_path(driver: &WebDriver) -> Result<String> {
    let breadcrumbs = driver
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

    if let Ok(title) = driver.title().await {
        return Ok(title);
    }

    Ok("Unknown Path".to_string())
}

fn extract_youtube_video_id(url: &str) -> Option<String> {
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

    println!("    Validating URL: {url}");

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    match client.head(url).send().await {
        Ok(response) => {
            let status = response.status().as_u16();
            if response.status().is_success() {
                println!("      URL valid ({status})");
                "Valid".to_string()
            } else if response.status().is_redirection() {
                println!("      URL redirects ({status})");
                "Redirect".to_string()
            } else {
                println!("      URL error ({status})");
                format!("Error {status}")
            }
        }
        Err(e) => {
            println!("      URL validation failed: {e}");
            "Invalid".to_string()
        }
    }
}

async fn extract_content_from_page(
    driver: &WebDriver,
    node_id: &str,
    validate_urls: bool,
) -> Result<Vec<ContentEntry>> {
    println!("  Extracting content from treeitem: {node_id}");

    println!("  Clicking treeitem to load content...");

    // Use retry wrapper to handle relogin dialogs
    retry_with_relogin(driver, || async {
        find_and_click_folder(driver, node_id).await
    }, 3).await?;

    println!("  Waiting for page to load after click...");
    support::sleep(Duration::from_secs(5)).await;

    println!("  Looking for dynamic content containers directly in the page...");

    let iframes = driver.find_all(By::Tag("iframe")).await?;
    println!("  Found {} iframes on the page", iframes.len());

    if !iframes.is_empty() {
        println!("  Attempting to enter first iframe...");
        match driver.enter_frame(0).await {
            Ok(_) => println!("  Successfully entered iframe"),
            Err(e) => println!("  Failed to enter iframe: {e}"),
        }
    }

    let dynamic_containers = driver
        .find_all(By::Css(".dynamicContent.dynamic-content-container-1"))
        .await?;

    println!("  Found {} dynamic containers", dynamic_containers.len());

    let breadcrumb_path = extract_breadcrumb_path(driver)
        .await
        .unwrap_or_else(|_| "Unknown Path".to_string());
    println!("  Breadcrumb path: {breadcrumb_path}");

    let mut entries = Vec::new();

    for (i, container) in dynamic_containers.iter().enumerate() {
        println!(
            "  Processing dynamic container {} of {}",
            i + 1,
            dynamic_containers.len()
        );
        container.scroll_into_view().await?;
        support::sleep(Duration::from_millis(500)).await;

        println!("    Looking for divs containing ExternalLinks paragraphs...");

        let link_container_divs = container
            .find_all(By::Css("div[data-__neos-fusion-path*='ExternalLinks']"))
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
                breadcrumb_path: breadcrumb_path.clone(),
                content_type: "ExternalLink".to_string(),
                url: String::new(),
                title: String::new(),
                author: String::new(),
                file_type: String::new(),
                size: String::new(),
                url_valid: String::new(),
            };

            println!("      Looking for URL...");
            if let Ok(url_element) = item.query(By::Css("p[property='typo3:url']")).first().await {
                if let Ok(url) = url_element.text().await {
                    entry.url = url.trim().to_string();
                    println!("      Found URL: {}", entry.url);
                }
            } else {
                println!("      No URL element found");
            }

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

            if validate_urls {
                entry.url_valid = validate_url(&entry.url).await;
            } else {
                entry.url_valid = "Skipped".to_string();
            }

            if !entry.url.is_empty() || !entry.title.is_empty() {
                entries.push(entry);
            }
        }

        println!("    Looking for YouTube content...");
        let youtube_container_divs = container
            .find_all(By::Css("div[data-__neos-fusion-path*='YouTube']"))
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
                breadcrumb_path: breadcrumb_path.clone(),
                content_type: "YouTube".to_string(),
                url: String::new(),
                title: String::new(),
                author: String::new(),
                file_type: "video".to_string(),
                size: String::new(),
                url_valid: String::new(),
            };

            println!("      Looking for YouTube iframe...");
            if let Ok(iframe_element) = item.query(By::Css("iframe")).first().await {
                if let Ok(Some(url)) = iframe_element.attr("src").await {
                    entry.url = url.trim().to_string();
                    println!("      Found YouTube URL: {}", entry.url);

                    if let Some(video_id) = extract_youtube_video_id(&entry.url) {
                        entry.title = format!("YouTube Video ({video_id})");
                    }
                }
            } else {
                println!("      No YouTube iframe found");
            }

            if validate_urls {
                entry.url_valid = validate_url(&entry.url).await;
            } else {
                entry.url_valid = "Skipped".to_string();
            }

            if !entry.url.is_empty() {
                entries.push(entry);
            }
        }
    }

    if !iframes.is_empty() {
        let _ = driver.enter_default_frame().await;
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
async fn add_tags(clear: bool, driver: &WebDriver) -> Result<()> {
    let tags = load_csv_data(TAGPATH).unwrap();
    let iframe = driver
        .query(By::Css(r#"iframe[name="neos-content-main"]"#))
        .first()
        .await?;
    iframe.clone().enter_frame().await?;

    let content_collection = driver
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
        driver.enter_default_frame().await?;

        let tag_textbox = driver
            .query(By::Css("#__neos__editor__property---Tags"))
            .first()
            .await?;

        driver
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

        let apply_button = driver
            .query(By::Css("#neos-Inspector-Apply"))
            .first()
            .await?;
        apply_button.click().await?;

        println!("{id} -> {value}");
        iframe.clone().enter_frame().await?;
        support::sleep(Duration::new(1, 0)).await;
    }
    driver.enter_default_frame().await?;
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

async fn bulk_extract_content(driver: &WebDriver) -> Result<()> {
    println!("\n=== Bulk Content Extraction ===");

    println!("Enter the treeitem ID to start extraction from:");
    let target_folder_id = read_line();

    if target_folder_id.is_empty() {
        println!("No folder ID provided. Using default: treeitem-c6643bf0-label");
        let target_folder_id = "treeitem-c6643bf0-label";
        return do_bulk_extract(driver, target_folder_id).await;
    }

    do_bulk_extract(driver, &target_folder_id).await
}

async fn do_bulk_extract(driver: &WebDriver, target_folder_id: &str) -> Result<()> {
    let validate_urls = ask_yes_no("Do you want to validate URLs? (This may take longer)");

    println!("Starting bulk extraction from folder: {target_folder_id}");
    if validate_urls {
        println!("URL validation is enabled - this will check if each URL is accessible");
    } else {
        println!("URL validation is disabled - URLs will be marked as 'Skipped'");
    }

    println!("Checking if target folder exists on current page...");

    // Check if we're on the right page and logged in
    let page_title = driver
        .title()
        .await
        .unwrap_or_else(|_| "Unknown".to_string());
    println!("Current page title: {}", page_title);

    // Wait a bit to ensure page is fully loaded
    println!("Waiting for page to load completely...");
    support::sleep(Duration::from_secs(3)).await;

    // Navigate to the target folder and expand it
    expand_folder_if_needed(driver, target_folder_id).await?;

    // Get all descendants (children, grandchildren, etc.) of the target folder
    let max_traversal_depth = 5;
    println!("Starting recursive traversal with max depth: {max_traversal_depth}");
    let child_ids = get_all_descendants(driver, target_folder_id, max_traversal_depth, 0).await?;
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
        if is_relogin_dialog_present(driver).await {
            println!("Relogin dialog detected before processing item {child_id}");
            match handle_relogin_dialog(driver).await {
                Ok(true) => println!("Relogin successful, continuing..."),
                Ok(false) => println!("Relogin failed, but continuing..."),
                Err(e) => println!("Error handling relogin: {e}, continuing..."),
            }
        }

        match extract_content_from_page(driver, child_id, validate_urls).await {
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

        support::sleep(Duration::from_millis(1500)).await;
    }

    // Also extract from the target folder itself
    println!("\nProcessing target folder: {target_folder_id}");
    match extract_content_from_page(driver, target_folder_id, validate_urls).await {
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

    // Write all entries to CSV
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

    println!("\n=== Bulk extraction complete! ===\n");
    println!("Total entries found: {}", all_entries.len());
    println!("Successfully processed pages: {successful}");
    println!("Failed pages: {failed}");
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
                KeyCode::Char('a') => add_tags(false, &spider.driver).await?,
                KeyCode::Char('c') => add_tags(true, &spider.driver).await?,
                KeyCode::Char('d') => {
                    bulk_extract_content(&spider.driver).await?;
                }
                _ => {}
            }
        }
    }

    spider.driver.quit().await?;
    Ok(())
}

// src/auth.rs
// Authentication and credentials handling

use anyhow::{Context, Result};
use std::path::PathBuf;
use std::{fs, time::Duration};
use thirtyfour::{support, By, WebDriver};

#[derive(serde::Deserialize)]
struct Credentials {
    username: String,
    password: String,
}

/// Get credentials from files
pub fn get_credentials() -> Result<(String, String)> {
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

/// Log in using the provided WebDriver
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

/// Check if relogin dialog is present
pub async fn is_relogin_dialog_present(driver: &WebDriver) -> bool {
    driver
        .find(By::Id("neos-ReloginDialog"))
        .await
        .is_ok()
}

/// Handle relogin dialog if present
pub async fn handle_relogin_dialog(driver: &WebDriver) -> Result<bool> {
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

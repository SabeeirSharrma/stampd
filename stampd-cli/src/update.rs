use anyhow::{Context, Result};
use flate2::read::GzDecoder;
use std::io::Read;

const GITHUB_REPO: &str = "SabeeirSharrma/stampd";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, serde::Deserialize)]
struct GitHubRelease {
    tag_name: String,
    #[allow(dead_code)]
    name: String,
    assets: Vec<GitHubAsset>,
}

#[derive(Debug, serde::Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
    #[allow(dead_code)]
    size: u64,
}

pub async fn cmd_update() -> Result<()> {
    println!("Checking for updates...");
    println!("Current version: v{}", CURRENT_VERSION);
    println!();

    // Fetch latest release from GitHub
    let client = reqwest::Client::builder()
        .user_agent(format!("stampd-cli/{}", CURRENT_VERSION))
        .build()
        .context("Failed to create HTTP client")?;

    let url = format!(
        "https://api.github.com/repos/{}/releases/latest",
        GITHUB_REPO
    );

    let response = client
        .get(&url)
        .send()
        .await
        .context("Failed to connect to GitHub. Check your internet connection.")?;

    if !response.status().is_success() {
        anyhow::bail!(
            "GitHub API returned status: {}. Repository may not exist or has no releases.",
            response.status()
        );
    }

    let release: GitHubRelease = response
        .json()
        .await
        .context("Failed to parse GitHub response")?;

    let latest_version = release.tag_name.trim_start_matches('v');
    println!("Latest version: v{}", latest_version);

    if latest_version == CURRENT_VERSION {
        println!();
        println!("You're already up to date!");
        return Ok(());
    }

    // Find the binary asset for current platform
    let asset = find_platform_asset(&release.assets)?;

    println!();
    println!("Downloading {}...", asset.name);

    // Download the asset
    let bytes = download_asset(&client, &asset.browser_download_url).await?;

    if bytes.is_empty() {
        anyhow::bail!("Downloaded file is empty. Please try again later.");
    }

    println!("Downloaded {} bytes", bytes.len());

    // Extract and replace binary
    let exe_path = std::env::current_exe().context("Failed to get current executable path")?;

    // Check if asset is tar.gz or raw binary
    if asset.name.ends_with(".tar.gz") {
        extract_and_replace(&bytes, &exe_path)?;
    } else {
        // Raw binary
        replace_binary(&bytes, &exe_path)?;
    }

    println!();
    println!(
        "Successfully updated from v{} to v{}",
        CURRENT_VERSION, latest_version
    );
    println!();

    Ok(())
}

fn find_platform_asset(assets: &[GitHubAsset]) -> Result<&GitHubAsset> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;

    // Build search patterns
    let patterns = if os == "linux" && arch == "x86_64" {
        vec![
            "stampd-engine-linux-x86_64",
            "stampd-engine-v*-linux-x86_64",
            "stampd-linux-x86_64",
        ]
    } else if os == "linux" && arch == "aarch64" {
        vec![
            "stampd-engine-linux-aarch64",
            "stampd-engine-v*-linux-aarch64",
            "stampd-linux-aarch64",
        ]
    } else {
        anyhow::bail!(
            "Unsupported platform: {}-{}. Currently only Linux x86_64 and aarch64 are supported.",
            os,
            arch
        );
    };

    // Try to find matching asset
    for pattern in &patterns {
        for asset in assets {
            if asset.name.contains(pattern) && !asset.name.ends_with(".sha256") {
                return Ok(asset);
            }
        }
    }

    // List available assets for debugging
    let asset_names: Vec<&str> = assets.iter().map(|a| a.name.as_str()).collect();
    anyhow::bail!(
        "No compatible binary found for {}-{}. Available assets: {:?}",
        os,
        arch,
        asset_names
    )
}

async fn download_asset(client: &reqwest::Client, url: &str) -> Result<Vec<u8>> {
    let response = client
        .get(url)
        .send()
        .await
        .context("Failed to download asset")?;

    if !response.status().is_success() {
        anyhow::bail!("Failed to download asset: HTTP {}", response.status());
    }

    let total_size = response.content_length().unwrap_or(0);
    let mut bytes = Vec::new();
    let mut stream = response;

    while let Some(chunk) = stream.chunk().await? {
        bytes.extend_from_slice(&chunk);
        if total_size > 0 {
            let progress = (bytes.len() as f64 / total_size as f64 * 100.0) as u32;
            print!("\rDownloading... {}%", progress);
        }
    }

    if total_size > 0 {
        println!("\rDownloading... 100%");
    }

    Ok(bytes)
}

fn extract_and_replace(archive_bytes: &[u8], exe_path: &std::path::Path) -> Result<()> {
    let decoder = GzDecoder::new(archive_bytes);
    let mut archive = tar::Archive::new(decoder);

    // Find the binary in the archive
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?;

        // Look for stampd-engine binary
        if path.file_name().is_some_and(|f| f == "stampd-engine")
            || path.to_string_lossy().contains("stampd-engine")
        {
            let mut binary_data = Vec::new();
            entry.read_to_end(&mut binary_data)?;

            if binary_data.is_empty() {
                anyhow::bail!("Extracted binary is empty");
            }

            // Verify it looks like a binary (ELF magic bytes on Linux)
            if cfg!(target_os = "linux")
                && binary_data.len() >= 4
                && &binary_data[..4] != b"\x7fELF"
            {
                anyhow::bail!("Extracted file does not appear to be a valid Linux binary");
            }

            return replace_binary(&binary_data, exe_path);
        }
    }

    anyhow::bail!("No stampd-engine binary found in archive")
}

fn replace_binary(new_binary: &[u8], exe_path: &std::path::Path) -> Result<()> {
    let dir = exe_path
        .parent()
        .context("Failed to get executable directory")?;

    // Create temp file in same directory (for atomic rename)
    let temp_path = dir.join(format!(".stampd-update-{}", std::process::id()));

    // Write new binary to temp file
    std::fs::write(&temp_path, new_binary).context("Failed to write new binary")?;

    // Make executable (Unix only)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o755);
        std::fs::set_permissions(&temp_path, perms)?;
    }

    // Atomic replace
    #[cfg(target_os = "linux")]
    {
        std::fs::rename(&temp_path, exe_path)
            .or_else(|_| {
                // If rename fails (e.g., different filesystem), try copy + delete
                std::fs::copy(&temp_path, exe_path)?;
                std::fs::remove_file(&temp_path)?;
                Ok::<(), std::io::Error>(())
            })
            .context("Failed to replace binary. You may need to run with sudo.")?;
    }

    #[cfg(not(target_os = "linux"))]
    {
        // On other platforms, try rename first, then copy
        if std::fs::rename(&temp_path, exe_path).is_err() {
            std::fs::copy(&temp_path, exe_path).context("Failed to replace binary")?;
            std::fs::remove_file(&temp_path).ok();
        }
    }

    Ok(())
}

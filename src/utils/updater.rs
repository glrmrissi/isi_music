use anyhow::{Context, Result, bail};

const REPO: &str = "glrmrissi/isi_music";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(serde::Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(serde::Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

fn asset_name_for_platform() -> &'static str {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        "isi-music-linux-x86_64"
    }
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    {
        "isi-music-linux-arm64"
    }
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        "isi-music-windows-x86_64.exe"
    }
    #[cfg(not(any(
        all(target_os = "linux", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "aarch64"),
        all(target_os = "windows", target_arch = "x86_64"),
    )))]
    {
        ""
    }
}

fn parse_version(tag: &str) -> Option<(u32, u32, u32)> {
    let v = tag.trim_start_matches('v');
    let parts: Vec<&str> = v.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    Some((
        parts[0].parse().ok()?,
        parts[1].parse().ok()?,
        parts[2].parse().ok()?,
    ))
}

fn is_newer(remote: &str, local: &str) -> bool {
    match (parse_version(remote), parse_version(local)) {
        (Some(r), Some(l)) => r > l,
        _ => false,
    }
}

async fn fetch_latest_release() -> Result<Release> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let client = reqwest::Client::builder()
        .user_agent("isi-music-updater")
        .timeout(std::time::Duration::from_secs(15))
        .build()?;
    let resp = client.get(&url).send().await?;
    if !resp.status().is_success() {
        bail!(
            "GitHub API returned {} -- check your internet connection",
            resp.status()
        );
    }
    let release: Release = resp.json().await.context("Failed to parse release JSON")?;
    Ok(release)
}

async fn download_asset(url: &str, dest: &std::path::Path) -> Result<()> {
    let client = reqwest::Client::builder()
        .user_agent("isi-music-updater")
        .timeout(std::time::Duration::from_secs(60))
        .build()?;
    let resp = client.get(url).send().await?;
    if !resp.status().is_success() {
        bail!("Download failed with status {}", resp.status());
    }
    let bytes = resp.bytes().await?;
    std::fs::write(dest, &bytes)
        .with_context(|| format!("Failed to write to {}", dest.display()))?;
    Ok(())
}

fn replace_binary(new_binary: &std::path::Path) -> Result<()> {
    let current_exe = std::env::current_exe().context("Could not determine current exe path")?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(new_binary, std::fs::Permissions::from_mode(0o755))?;
        std::fs::rename(new_binary, &current_exe)
            .with_context(|| format!("Failed to replace {}", current_exe.display()))?;
    }

    #[cfg(windows)]
    {
        // Windows can't overwrite a running .exe -- rename it to .old first
        let old = current_exe.with_extension("exe.old");
        if old.exists() {
            let _ = std::fs::remove_file(&old);
        }
        std::fs::rename(&current_exe, &old).context("Failed to rename current exe")?;
        std::fs::rename(new_binary, &current_exe)
            .with_context(|| format!("Failed to write new exe to {}", current_exe.display()))?;
        // .old will be cleaned up on next launch
        let _ = std::fs::remove_file(&old);
    }

    Ok(())
}

/// Clean up any stale `.exe.old` files from a previous update (Windows only).
pub fn cleanup_old_binary() {
    #[cfg(windows)]
    {
        if let Ok(exe) = std::env::current_exe() {
            let old = exe.with_extension("exe.old");
            if old.exists() {
                let _ = std::fs::remove_file(&old);
            }
        }
    }
}

pub async fn run() -> Result<()> {
    cleanup_old_binary();

    println!();
    println!("  isi-music - Update");
    println!("  {}", "-".repeat(50));
    println!();
    println!("  Current version: v{CURRENT_VERSION}");
    println!("  Checking for updates...");

    let release = fetch_latest_release().await?;
    let remote_tag = &release.tag_name;

    println!("  Latest release:  {remote_tag}");
    println!();

    if !is_newer(remote_tag, CURRENT_VERSION) {
        println!("  Already up to date.");
        println!();
        return Ok(());
    }

    let asset_name = asset_name_for_platform();
    let asset = release
        .assets
        .iter()
        .find(|a| a.name == asset_name)
        .with_context(|| {
            format!("No binary found for your platform ({asset_name}). Download manually from https://github.com/{REPO}/releases")
        })?;

    println!("  New version available! Downloading {asset_name}...");
    let tmp = std::env::temp_dir().join(format!("isi-music-update-{}", std::process::id()));
    download_asset(&asset.browser_download_url, &tmp).await?;

    println!("  Installing...");
    replace_binary(&tmp)?;

    println!();
    println!("  Updated: v{CURRENT_VERSION} -> {remote_tag}");
    println!("  Restart isi-music to use the new version.");
    println!();

    Ok(())
}

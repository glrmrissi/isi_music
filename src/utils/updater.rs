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
        .pool_max_idle_per_host(1)
        .pool_idle_timeout(std::time::Duration::from_secs(10))
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

async fn download_asset(url: &str) -> Result<Vec<u8>> {
    let client = reqwest::Client::builder()
        .user_agent("isi-music-updater")
        .timeout(std::time::Duration::from_secs(60))
        .pool_max_idle_per_host(1)
        .pool_idle_timeout(std::time::Duration::from_secs(10))
        .build()?;
    let resp = client.get(url).send().await?;
    if !resp.status().is_success() {
        bail!("Download failed with status {}", resp.status());
    }
    Ok(resp.bytes().await?.to_vec())
}

async fn fetch_checksums(release: &Release) -> Result<String> {
    let asset = release
        .assets
        .iter()
        .find(|a| a.name == "checksums.sha256")
        .with_context(|| {
            format!(
                "Release has no checksums.sha256 asset; refusing to install an unverified binary. \
                 Download manually from https://github.com/{REPO}/releases"
            )
        })?;
    let bytes = download_asset(&asset.browser_download_url).await?;
    String::from_utf8(bytes).context("checksums.sha256 is not valid UTF-8")
}

/// Parse `sha256sum` output (`<hash>  <name>` or `<hash> *<name>`).
fn expected_sha256(checksums: &str, asset_name: &str) -> Option<String> {
    for line in checksums.lines() {
        let mut parts = line.split_whitespace();
        let Some(hash) = parts.next() else { continue };
        let Some(file) = parts.next() else { continue };
        if file.trim_start_matches('*') == asset_name {
            return Some(hash.to_lowercase());
        }
    }
    None
}

fn sha256_hex(data: &[u8]) -> String {
    use sha2::Digest;
    let hash = sha2::Sha256::digest(data);
    hash.iter().map(|b| format!("{b:02x}")).collect()
}

/// Write `contents` to a fresh, unpredictable path in the system temp dir.
/// `create_new` maps to O_EXCL: it refuses to follow or overwrite a file
/// (including a symlink) that another local user pre-created there.
fn write_temp_binary(contents: &[u8]) -> Result<std::path::PathBuf> {
    let path = std::env::temp_dir().join(format!(
        "isi-music-update-{}-{:016x}",
        std::process::id(),
        rand::random::<u64>()
    ));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .with_context(|| format!("Failed to create {}", path.display()))?;
    std::io::Write::write_all(&mut file, contents)
        .with_context(|| format!("Failed to write to {}", path.display()))?;
    Ok(path)
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
    let bytes = download_asset(&asset.browser_download_url).await?;

    println!("  Verifying checksum...");
    // Checksum file is same-origin as the binary: guards corruption and
    // MITM outside GitHub; signed artifacts would be needed for full trust.
    let checksums = fetch_checksums(&release).await?;
    let expected = expected_sha256(&checksums, asset_name).with_context(|| {
        format!("No checksum for {asset_name} in checksums.sha256; refusing to install")
    })?;
    let actual = sha256_hex(&bytes);
    if actual != expected {
        bail!("Checksum mismatch for {asset_name}: expected {expected}, got {actual}");
    }

    let tmp = write_temp_binary(&bytes)?;

    println!("  Installing...");
    if let Err(e) = replace_binary(&tmp) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }

    println!();
    println!("  Updated: v{CURRENT_VERSION} -> {remote_tag}");
    println!("  Restart isi-music to use the new version.");
    println!();

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expected_sha256_matches_asset_line() {
        let checksums = "aaaa1111  isi-music-linux-x86_64\nbbbb2222  isi-music-linux-arm64\n";
        assert_eq!(
            expected_sha256(checksums, "isi-music-linux-arm64").as_deref(),
            Some("bbbb2222")
        );
    }

    #[test]
    fn expected_sha256_handles_binary_marker_and_case() {
        let checksums = "AAAA1111 *isi-music-windows-x86_64.exe\n";
        assert_eq!(
            expected_sha256(checksums, "isi-music-windows-x86_64.exe").as_deref(),
            Some("aaaa1111")
        );
    }

    #[test]
    fn expected_sha256_returns_none_for_missing_asset() {
        let checksums = "aaaa1111  isi-music-linux-x86_64\n";
        assert!(expected_sha256(checksums, "isi-music-macos").is_none());
    }

    #[test]
    fn sha256_hex_known_vector() {
        // sha256("abc") from the FIPS test vectors
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}

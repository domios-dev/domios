use serde::{Deserialize, Serialize};
use super::InstallMethod;

const GITHUB_API: &str = "https://api.github.com/repos/rollacode/codeterm/releases/latest";
const GITHUB_REPO: &str = "rollacode/codeterm";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub update_available: bool,
    pub install_method: InstallMethod,
    /// Command to run for package manager installs
    pub update_command: Option<String>,
    /// Direct download URL for the platform-specific asset
    pub download_url: Option<String>,
    /// GitHub release page URL
    pub release_url: String,
    /// Release notes (markdown body)
    pub release_notes: Option<String>,
    pub published_at: Option<String>,
}

pub async fn check_for_update(install_method: &InstallMethod) -> Result<UpdateInfo, String> {
    let client = reqwest::Client::builder()
        .user_agent("CodeTerm-Updater")
        .build()
        .map_err(|e| e.to_string())?;

    let resp: serde_json::Value = client
        .get(GITHUB_API)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch release: {}", e))?
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    let tag = resp["tag_name"]
        .as_str()
        .ok_or("No tag_name in GitHub response")?;
    let latest_str = tag.strip_prefix('v').unwrap_or(tag);
    let current_str = env!("CARGO_PKG_VERSION");

    let current = semver::Version::parse(current_str).map_err(|e| e.to_string())?;
    let latest = semver::Version::parse(latest_str).map_err(|e| e.to_string())?;

    let update_available = latest > current;
    let update_command = build_update_command(install_method, latest_str);
    let download_url = resolve_download_url(install_method, &resp);

    Ok(UpdateInfo {
        current_version: current_str.to_string(),
        latest_version: latest_str.to_string(),
        update_available,
        install_method: install_method.clone(),
        update_command,
        download_url,
        release_url: resp["html_url"]
            .as_str()
            .unwrap_or("")
            .to_string(),
        release_notes: resp["body"].as_str().map(|s| s.to_string()),
        published_at: resp["published_at"].as_str().map(|s| s.to_string()),
    })
}

fn build_update_command(method: &InstallMethod, version: &str) -> Option<String> {
    match method {
        InstallMethod::Brew => Some("brew upgrade --cask codeterm".to_string()),
        InstallMethod::Scoop => Some("scoop update codeterm".to_string()),
        InstallMethod::Deb => Some(format!(
            "curl -sLO https://github.com/{}/releases/download/v{}/CodeTerm_{}_amd64.deb && sudo dpkg -i CodeTerm_{}_amd64.deb",
            GITHUB_REPO, version, version, version
        )),
        InstallMethod::Rpm => Some(format!(
            "curl -sLO https://github.com/{}/releases/download/v{}/CodeTerm-{}-1.x86_64.rpm && sudo rpm -U CodeTerm-{}-1.x86_64.rpm",
            GITHUB_REPO, version, version, version
        )),
        _ => None,
    }
}

/// Find the platform-appropriate download URL from release assets.
fn resolve_download_url(method: &InstallMethod, release: &serde_json::Value) -> Option<String> {
    let assets = release["assets"].as_array()?;
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;

    let suffix = match (method, os, arch) {
        (InstallMethod::Dmg, "macos", "aarch64") => "_aarch64.dmg",
        (InstallMethod::Dmg, "macos", _) => "_x64.dmg",
        (InstallMethod::Brew, "macos", "aarch64") => "_aarch64.dmg",
        (InstallMethod::Brew, "macos", _) => "_x64.dmg",
        (InstallMethod::Nsis, "windows", _) | (InstallMethod::Scoop, "windows", _) => "_x64-setup.exe",
        (InstallMethod::Msi, "windows", _) => "_x64_en-US.msi",
        (InstallMethod::AppImage, "linux", _) => "_amd64.AppImage",
        (InstallMethod::Deb, "linux", _) => "_amd64.deb",
        (InstallMethod::Rpm, "linux", _) => ".x86_64.rpm",
        _ => return None,
    };

    assets
        .iter()
        .find(|a| {
            a["name"]
                .as_str()
                .map(|n| n.ends_with(suffix))
                .unwrap_or(false)
        })
        .and_then(|a| a["browser_download_url"].as_str().map(|s| s.to_string()))
}

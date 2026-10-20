use super::InstallMethod;
use codeterm_service_core::update_state::UpdateInfo;

// Anonymous users check a public releases endpoint.
// Do not bake updater tokens into shipped binaries; that breaks public download flows.
const GITHUB_API: &str = "https://api.github.com/repos/rollacode/codeterm-releases/releases/latest";
pub(super) const GITHUB_REPO: &str = "rollacode/codeterm-releases";

fn latest_release_request(client: &reqwest::Client) -> reqwest::RequestBuilder {
    client
        .get(GITHUB_API)
        .query(&[("codeterm_cache_bust", uuid::Uuid::new_v4().to_string())])
        .header(reqwest::header::CACHE_CONTROL, "no-cache, no-store")
        .header(reqwest::header::PRAGMA, "no-cache")
}

pub async fn latest_release_tag() -> Result<String, String> {
    if codeterm_plugins::update_source::fixture_root().is_some() {
        let manifest = super::manifest::fetch("https://fixtures.invalid/latest.json").await?;
        return Ok(format!("v{}", manifest.version));
    }
    let client = reqwest::Client::builder()
        .user_agent("CodeTerm-Updater")
        .build()
        .map_err(|e| e.to_string())?;
    let release = latest_release_request(&client)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch release: {}", e))?;
    if !release.status().is_success() {
        return Err(format!(
            "Failed to fetch release: HTTP {}",
            release.status()
        ));
    }
    let release: serde_json::Value = release
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;
    release["tag_name"]
        .as_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| "No tag_name in GitHub response".to_string())
}

pub async fn check_for_update(install_method: &InstallMethod) -> Result<UpdateInfo, String> {
    let endpoint = super::channel::default_endpoints_for_install_method(
        super::Channel::Stable,
        install_method,
    )
    .into_iter()
    .next()
    .ok_or("Missing updater endpoint")?;
    let manifest = super::manifest::fetch(&endpoint).await?;
    let current_str = codeterm_build_info::version();
    let compatible = super::manifest::current_platform_key()
        .is_some_and(|key| manifest.platforms.contains_key(key));
    let availability = codeterm_plugins::AvailabilityItem::release(
        "app",
        codeterm_plugins::UpdateKind::App,
        current_str,
        &manifest.version,
        compatible,
    );
    Ok(UpdateInfo {
        current_version: current_str.into(),
        latest_version: manifest.version.clone(),
        update_available: availability.state
            == codeterm_plugins::AvailabilityState::UpdateAvailable,
        install_method: install_method.clone(),
        update_command: build_update_command(install_method, &manifest.version),
        download_url: super::manifest::current_platform_key()
            .and_then(|key| manifest.platforms.get(key))
            .map(|platform| platform.url.clone()),
        release_url: format!(
            "https://github.com/{GITHUB_REPO}/releases/tag/v{}",
            manifest.version
        ),
        release_notes: manifest.notes,
        published_at: manifest.pub_date,
        availability,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latest_release_request_bypasses_mutable_github_cache() {
        let request = latest_release_request(&reqwest::Client::new())
            .build()
            .unwrap();
        assert_eq!(
            request.headers()[reqwest::header::CACHE_CONTROL],
            "no-cache, no-store"
        );
        assert_eq!(request.headers()[reqwest::header::PRAGMA], "no-cache");
        assert!(request
            .url()
            .query_pairs()
            .any(|(key, value)| key == "codeterm_cache_bust" && !value.is_empty()));
    }
}

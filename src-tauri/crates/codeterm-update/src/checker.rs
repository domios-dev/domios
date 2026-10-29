use super::InstallMethod;
use crate::update_state::UpdateInfo;

// Anonymous public endpoints only; never bake updater tokens into shipped binaries.
// From 1.16.0 releases publish to the public repository; the newest version across both wins.
pub const UPDATE_REPOSITORIES: [&str; 2] = ["domios-dev/domios", "rollacode/codeterm-releases"];
pub(super) const GITHUB_REPO: &str = UPDATE_REPOSITORIES[0];

/// The newest stable release found across the update repositories.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LatestRelease {
    pub repository: &'static str,
    pub tag: String,
}

fn latest_release_request(client: &reqwest::Client, repository: &str) -> reqwest::RequestBuilder {
    client
        .get(format!(
            "https://api.github.com/repos/{repository}/releases/latest"
        ))
        .query(&[("codeterm_cache_bust", uuid::Uuid::new_v4().to_string())])
        .header(reqwest::header::CACHE_CONTROL, "no-cache, no-store")
        .header(reqwest::header::PRAGMA, "no-cache")
}

pub async fn latest_release_tag() -> Result<LatestRelease, String> {
    if codeterm_plugins::update_source::fixture_root().is_some() {
        let manifest =
            super::manifest::fetch("https://fixtures.invalid/domios-latest.json").await?;
        return Ok(LatestRelease {
            repository: GITHUB_REPO,
            tag: format!("v{}", manifest.version),
        });
    }
    let client = reqwest::Client::builder()
        .user_agent("Domios-Updater")
        .build()
        .map_err(|e| e.to_string())?;
    let mut found = Vec::new();
    let mut errors = Vec::new();
    for repository in UPDATE_REPOSITORIES {
        match repository_latest_tag(&client, repository).await {
            Ok(tag) => found.push(LatestRelease { repository, tag }),
            Err(error) => errors.push(format!("{repository}: {error}")),
        }
    }
    newest_release(found).ok_or_else(|| format!("Failed to fetch release: {}", errors.join("; ")))
}

async fn repository_latest_tag(
    client: &reqwest::Client,
    repository: &str,
) -> Result<String, String> {
    let release = latest_release_request(client, repository)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !release.status().is_success() {
        return Err(format!("HTTP {}", release.status()));
    }
    let release: serde_json::Value = release.json().await.map_err(|e| e.to_string())?;
    release["tag_name"]
        .as_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| "no tag_name".to_string())
}

/// Highest semantic version wins; the earlier repository wins a tie, and unparsable tags never win.
pub fn newest_release(found: Vec<LatestRelease>) -> Option<LatestRelease> {
    let mut best: Option<(semver::Version, LatestRelease)> = None;
    for release in found {
        let Ok(candidate) = semver::Version::parse(release.tag.trim_start_matches('v')) else {
            continue;
        };
        if best
            .as_ref()
            .is_none_or(|(current, _)| candidate > *current)
        {
            best = Some((candidate, release));
        }
    }
    best.map(|(_, release)| release)
}

pub async fn check_for_update(install_method: &InstallMethod) -> Result<UpdateInfo, String> {
    if let Some(backend) = crate::headless::backend() {
        return backend.check().await.map_err(|e| e.to_string());
    }
    check_for_update_at(install_method, None).await
}

pub async fn check_for_update_at(
    install_method: &InstallMethod,
    manifest_url: Option<&str>,
) -> Result<UpdateInfo, String> {
    let endpoint = match manifest_url {
        Some(url) => url.to_string(),
        None => {
            let latest = latest_release_tag().await?;
            super::channel::immutable_endpoint_for_release(
                latest.repository,
                &latest.tag,
                super::Channel::Stable,
                install_method,
            )
        }
    };
    let endpoint = endpoint.as_str();
    let manifest = super::manifest::fetch(endpoint).await.map_err(|error| {
        log::error!("[updater] GET {endpoint} failed: {error}");
        format!("GET {endpoint}: {error}")
    })?;
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
    let download_url = super::manifest::current_platform_key()
        .and_then(|key| manifest.platforms.get(key))
        .map(|platform| platform.url.clone());
    Ok(UpdateInfo {
        current_version: current_str.into(),
        latest_version: manifest.version.clone(),
        update_available: availability.state
            == codeterm_plugins::AvailabilityState::UpdateAvailable,
        install_method: install_method.clone(),
        update_command: build_update_command(
            install_method,
            &manifest.version,
            download_url.as_deref(),
        ),
        download_url,
        release_url: format!(
            "https://github.com/{GITHUB_REPO}/releases/tag/v{}",
            manifest.version
        ),
        release_notes: manifest.notes,
        published_at: manifest.pub_date,
        availability,
    })
}

fn build_update_command(
    method: &InstallMethod,
    version: &str,
    _download_url: Option<&str>,
) -> Option<String> {
    match method {
        InstallMethod::Brew => Some("brew upgrade --cask domios-dev/domios/domios".to_string()),
        InstallMethod::Scoop => Some("scoop update domios".to_string()),
        InstallMethod::Deb => None,
        InstallMethod::Rpm => Some(format!(
            "curl -sLO https://github.com/{}/releases/download/v{}/Domios-{}-1.x86_64.rpm && sudo rpm -U Domios-{}-1.x86_64.rpm",
            GITHUB_REPO, version, version, version
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests;

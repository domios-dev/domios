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

#[derive(Clone)]
struct ResolvedManifest {
    release: LatestRelease,
    manifest: super::manifest::Manifest,
    refresh_error: Option<String>,
}

mod cache;

pub(crate) async fn resolve_release(
    method: &InstallMethod,
    channel: super::Channel,
) -> Result<LatestRelease, String> {
    resolve_feed(method, channel)
        .await
        .map(|resolved| resolved.release)
}

pub async fn resolve_manifest(
    method: &InstallMethod,
    channel: super::Channel,
    manifest_url: Option<&str>,
) -> Result<super::manifest::Manifest, String> {
    if let Some(url) = manifest_url {
        return super::manifest::fetch(url).await;
    }
    resolve_feed(method, channel)
        .await
        .map(|resolved| resolved.manifest)
}

async fn resolve_feed(
    method: &InstallMethod,
    channel: super::Channel,
) -> Result<ResolvedManifest, String> {
    let root = codeterm_plugins::update_source::fixture_root()
        .map(|root| root.join("manifest-cache"))
        .unwrap_or_else(|| codeterm_paths::data_dir_path().join("update-manifests"));
    resolve_with(method, channel, root, |endpoint| async move {
        super::manifest::fetch(&endpoint).await
    })
    .await
}

async fn resolve_with<F, Fut>(
    method: &InstallMethod,
    channel: super::Channel,
    root: std::path::PathBuf,
    mut fetch: F,
) -> Result<ResolvedManifest, String>
where
    F: FnMut(String) -> Fut,
    Fut: std::future::Future<Output = Result<super::manifest::Manifest, String>>,
{
    let key = super::channel::manifest_filename_for_install_method(channel, method);
    let mut found = Vec::new();
    let mut errors = Vec::new();
    let endpoints = super::channel::default_endpoints_for_install_method(channel, method);
    for (repository, endpoint) in UPDATE_REPOSITORIES.into_iter().zip(endpoints) {
        match fetch(endpoint).await.and_then(|manifest| {
            semver::Version::parse(&manifest.version).map_err(|e| e.to_string())?;
            Ok(manifest)
        }) {
            Ok(manifest) => found.push(ResolvedManifest {
                release: LatestRelease {
                    repository,
                    tag: format!("v{}", manifest.version),
                },
                manifest,
                refresh_error: None,
            }),
            Err(error) => errors.push(format!("{repository}: {error}")),
        }
    }
    let best = newest_release(found.iter().map(|item| item.release.clone()).collect());
    let fresh = best.and_then(|best| found.into_iter().find(|item| item.release == best));
    tokio::task::spawn_blocking(move || cache::publish(&root, key, fresh, &errors))
        .await
        .map_err(|error| error.to_string())?
}

fn reuse_manifest(
    fresh: Option<ResolvedManifest>,
    cached: Option<ResolvedManifest>,
    errors: &[String],
) -> Result<ResolvedManifest, String> {
    let error = format!("Failed to fetch release: {}", errors.join("; "));
    match (fresh, cached) {
        (Some(fresh), Some(mut cached)) => {
            let fresh_version =
                semver::Version::parse(&fresh.manifest.version).map_err(|e| e.to_string())?;
            let cached_version =
                semver::Version::parse(&cached.manifest.version).map_err(|e| e.to_string())?;
            if cached_version > fresh_version
                || (cached_version == fresh_version && !errors.is_empty())
            {
                cached.refresh_error = Some(if errors.is_empty() {
                    format!(
                        "Release feed returned {}; keeping known {}",
                        fresh_version, cached_version
                    )
                } else {
                    error
                });
                Ok(cached)
            } else {
                Ok(fresh)
            }
        }
        (Some(fresh), None) => Ok(fresh),
        (None, Some(mut cached)) => {
            cached.refresh_error = Some(error);
            Ok(cached)
        }
        (None, None) => Err(error),
    }
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
    if let Some(error) = crate::container::refusal() {
        return Err(serde_json::to_string(&error).unwrap_or_else(|_| error.to_string()));
    }
    if let Some(backend) = crate::headless::backend() {
        return backend.check().await.map_err(|e| e.to_string());
    }
    check_for_update_at(install_method, None).await
}

pub async fn check_for_update_at(
    install_method: &InstallMethod,
    manifest_url: Option<&str>,
) -> Result<UpdateInfo, String> {
    check_with_diagnostic(install_method, manifest_url)
        .await
        .map(|(info, _)| info)
}

pub async fn check_with_diagnostic(
    install_method: &InstallMethod,
    manifest_url: Option<&str>,
) -> Result<(UpdateInfo, Option<String>), String> {
    let (manifest, refresh_error) = match manifest_url {
        Some(url) => (super::manifest::fetch(url).await?, None),
        None => {
            let resolved = resolve_feed(install_method, super::Channel::Stable).await?;
            (resolved.manifest, resolved.refresh_error)
        }
    };
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
    Ok((
        UpdateInfo {
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
        },
        refresh_error,
    ))
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

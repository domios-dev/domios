use super::InstallMethod;
use crate::check_throttle::{CheckThrottle, CheckTrigger};
use crate::update_state::UpdateInfo;

// Anonymous public endpoint only; never bake updater tokens into shipped binaries.
pub const UPDATE_REPOSITORY: &str = "domios-dev/domios";

/// An automatic check inside its interval with nothing known yet; not a failed source.
pub const CHECK_DEFERRED: &str = "update check deferred by its interval";

/// The newest stable release published by the update repository.
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
    resolve_with(method, channel, manifest_root(), fetch_explicit)
        .await
        .map(|resolved| resolved.release)
}

/// The install path: resolved fresh, whatever the automatic interval says.
pub async fn resolve_manifest(
    method: &InstallMethod,
    channel: super::Channel,
    manifest_url: Option<&str>,
) -> Result<super::manifest::Manifest, String> {
    if let Some(url) = manifest_url {
        return super::manifest::fetch_explicit(url).await;
    }
    resolve_with(method, channel, manifest_root(), fetch_explicit)
        .await
        .map(|resolved| resolved.manifest)
}

async fn fetch_explicit(endpoint: String) -> Result<super::manifest::Manifest, String> {
    super::manifest::fetch_explicit(&endpoint).await
}

async fn fetch_automatic(endpoint: String) -> Result<super::manifest::Manifest, String> {
    super::manifest::fetch(&endpoint).await
}

fn manifest_root() -> std::path::PathBuf {
    codeterm_plugins::update_source::fixture_root()
        .map(|root| root.join("manifest-cache"))
        .unwrap_or_else(|| codeterm_paths::data_dir_path().join("update-manifests"))
}

fn now_ms() -> u64 {
    codeterm_plugins::update_source::github_release::now_ms()
}

/// Whether an automatic check would reach the network now; dev fixtures are never throttled.
pub async fn automatic_check_due() -> bool {
    if codeterm_plugins::update_source::fixture_root().is_some() {
        return true;
    }
    let throttle = CheckThrottle::in_root(&manifest_root());
    let now = now_ms();
    tokio::task::spawn_blocking(move || throttle.due(now))
        .await
        .unwrap_or(true)
}

async fn admit_check(root: std::path::PathBuf, trigger: CheckTrigger, now: u64) -> bool {
    tokio::task::spawn_blocking(move || CheckThrottle::in_root(&root).admit(trigger, now))
        .await
        .unwrap_or(true)
}

/// Inside the interval an automatic check answers from the last published manifest it saw.
async fn gated_manifest<F, Fut>(
    method: &InstallMethod,
    channel: super::Channel,
    trigger: CheckTrigger,
    root: std::path::PathBuf,
    now: u64,
    fetch: F,
) -> Result<ResolvedManifest, String>
where
    F: FnMut(String) -> Fut,
    Fut: std::future::Future<Output = Result<super::manifest::Manifest, String>>,
{
    if admit_check(root.clone(), trigger, now).await {
        return resolve_with(method, channel, root, fetch).await;
    }
    let key = super::channel::manifest_filename_for_install_method(channel, method);
    tokio::task::spawn_blocking(move || cache::read_cached(&root, key))
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| CHECK_DEFERRED.to_string())
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
    let endpoint = super::channel::default_endpoint_for_install_method(channel, method);
    let mut errors = Vec::new();
    let fresh = match fetch(endpoint).await.and_then(|manifest| {
        semver::Version::parse(&manifest.version).map_err(|e| e.to_string())?;
        Ok(manifest)
    }) {
        Ok(manifest) => Some(ResolvedManifest {
            release: LatestRelease {
                repository: UPDATE_REPOSITORY,
                tag: format!("v{}", manifest.version),
            },
            manifest,
            refresh_error: None,
        }),
        Err(error) => {
            errors.push(format!("{UPDATE_REPOSITORY}: {error}"));
            None
        }
    };
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

pub async fn check_for_update(
    install_method: &InstallMethod,
    trigger: CheckTrigger,
) -> Result<UpdateInfo, String> {
    if let Some(error) = crate::container::refusal() {
        return Err(serde_json::to_string(&error).unwrap_or_else(|_| error.to_string()));
    }
    if let Some(backend) = crate::headless::backend() {
        if !admit_check(manifest_root(), trigger, now_ms()).await {
            return Err(CHECK_DEFERRED.into());
        }
        return backend.check().await.map_err(|e| e.to_string());
    }
    check_with_diagnostic(install_method, None, trigger)
        .await
        .map(|(info, _)| info)
}

pub async fn check_with_diagnostic(
    install_method: &InstallMethod,
    manifest_url: Option<&str>,
    trigger: CheckTrigger,
) -> Result<(UpdateInfo, Option<String>), String> {
    // Dev fixture drills exercise discovery repeatedly and never reach the network.
    let trigger = if codeterm_plugins::update_source::fixture_root().is_some() {
        CheckTrigger::Explicit
    } else {
        trigger
    };
    let (manifest, refresh_error) = match manifest_url {
        Some(url) => (super::manifest::fetch_explicit(url).await?, None),
        None => {
            let channel = super::Channel::Stable;
            let (root, now) = (manifest_root(), now_ms());
            let resolved = match trigger {
                CheckTrigger::Explicit => {
                    gated_manifest(install_method, channel, trigger, root, now, fetch_explicit)
                        .await?
                }
                CheckTrigger::Automatic => {
                    gated_manifest(install_method, channel, trigger, root, now, fetch_automatic)
                        .await?
                }
            };
            (resolved.manifest, resolved.refresh_error)
        }
    };
    let check_path = manifest.served_by.map(|served| served.as_str().to_string());
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
                "https://github.com/{UPDATE_REPOSITORY}/releases/tag/v{}",
                manifest.version
            ),
            release_notes: manifest.notes,
            published_at: manifest.pub_date,
            availability,
            check_path,
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
            UPDATE_REPOSITORY, version, version, version
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests;

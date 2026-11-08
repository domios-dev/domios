use super::{
    manifest::{self},
    store::{Store, MAX_ARCHIVE_BYTES},
};
use codeterm_plugins::update_source::github_release;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Default)]
pub struct CheckJob(AtomicBool);

pub struct CheckLease<'a>(&'a CheckJob);

impl CheckJob {
    pub fn acquire(&self) -> Result<CheckLease<'_>, String> {
        self.0
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| CheckLease(self))
            .map_err(|_| "Frontend check already in flight".into())
    }
}

impl Drop for CheckLease<'_> {
    fn drop(&mut self) {
        self.0 .0.store(false, Ordering::Release);
    }
}

pub struct Discovery {
    pub(crate) manifest: manifest::Manifest,
    metadata: Vec<u8>,
    signature: String,
}

pub fn download_and_stage(
    store: &Store,
    discovery: &Discovery,
    floor: &semver::Version,
    key: &str,
    api: u32,
    allow_loopback: bool,
) -> Result<manifest::VerifiedManifest, String> {
    let client = reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() >= 5 {
                attempt.error("Frontend redirect limit")
            } else if manifest::validate_redirect_url(attempt.url().as_str(), allow_loopback)
                .is_err()
            {
                attempt.error("Unsafe frontend redirect")
            } else {
                attempt.follow()
            }
        }))
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())?;
    stage_with(
        store,
        discovery,
        floor,
        key,
        api,
        allow_loopback,
        |url, limit| download(&client, url, limit),
    )
}

pub(crate) fn stage_with(
    store: &Store,
    discovery: &Discovery,
    floor: &semver::Version,
    key: &str,
    api: u32,
    allow_loopback: bool,
    fetch: impl FnOnce(&str, usize) -> Result<Vec<u8>, String>,
) -> Result<manifest::VerifiedManifest, String> {
    let Discovery {
        manifest,
        metadata,
        signature,
    } = discovery;
    if !manifest::newer_than(&manifest.version, floor) {
        return Err("Frontend release is not newer than the running frontend".into());
    }
    if !manifest.compatible(api) {
        return Err("Incompatible frontend API".into());
    }
    manifest::validate_url(&manifest.url, allow_loopback)?;
    let archive = fetch(&manifest.url, MAX_ARCHIVE_BYTES)?;
    let verified = manifest::verify(metadata, signature, &archive, key, api, allow_loopback)?;
    store.install(&verified, signature, &archive)?;
    Ok(verified)
}

fn download(
    client: &reqwest::blocking::Client,
    url: &str,
    limit: usize,
) -> Result<Vec<u8>, String> {
    use std::io::Read;
    if codeterm_plugins::update_source::fixture_root().is_some() {
        return codeterm_plugins::update_source::discovery_source().fetch(url, limit);
    }
    let response = match client
        .get(url)
        .send()
        .and_then(|response| response.error_for_status())
    {
        Ok(response) => response,
        Err(error) if github_release::is_outage_error(&error) => {
            return github_release::official_fallback(url, limit, &error.to_string());
        }
        Err(error) => return Err(error.to_string()),
    };
    if response
        .content_length()
        .is_some_and(|size| size > limit as u64)
    {
        return Err("Download exceeds limit".into());
    }
    let mut bytes = Vec::new();
    if let Err(error) = response.take(limit as u64 + 1).read_to_end(&mut bytes) {
        return github_release::official_fallback(url, limit, &error.to_string());
    }
    if bytes.len() > limit {
        return Err("Download exceeds limit".into());
    }
    Ok(bytes)
}

pub fn discover(discovery: &str, key: &str, allow_loopback: bool) -> Result<Discovery, String> {
    let source = codeterm_plugins::update_source::discovery_source();
    discover_with(discovery, key, allow_loopback, |url, limit| {
        source.fetch(url, limit)
    })
}

pub(crate) fn discover_with(
    discovery: &str,
    key: &str,
    allow_loopback: bool,
    fetch: impl Fn(&str, usize) -> Result<Vec<u8>, String>,
) -> Result<Discovery, String> {
    manifest::validate_url(discovery, allow_loopback)?;
    let metadata = fetch(discovery, 16384)?;
    let signature = String::from_utf8(fetch(&format!("{discovery}.minisig"), 16384)?)
        .map_err(|e| e.to_string())?;
    let manifest = manifest::verify_discovery(&metadata, &signature, key, allow_loopback)?;
    Ok(Discovery {
        manifest,
        metadata,
        signature,
    })
}

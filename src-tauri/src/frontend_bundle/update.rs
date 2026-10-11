use super::{
    manifest::{self},
    store::{Store, MAX_ARCHIVE_BYTES},
};
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

pub fn download_and_stage(
    store: &Store,
    discovery: &str,
    floor: &semver::Version,
    key: &str,
    api: u32,
    allow_loopback: bool,
) -> Result<manifest::VerifiedManifest, String> {
    manifest::validate_url(discovery, allow_loopback)?;
    let client = reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() >= 5 {
                attempt.error("Frontend redirect limit")
            } else if manifest::validate_url(attempt.url().as_str(), allow_loopback).is_err() {
                attempt.error("Unsafe frontend redirect")
            } else {
                attempt.follow()
            }
        }))
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())?;
    let metadata = download(&client, discovery, 16384)?;
    let signature = String::from_utf8(download(&client, &format!("{discovery}.minisig"), 16384)?)
        .map_err(|e| e.to_string())?;
    let manifest = manifest::verify_metadata(&metadata, &signature, key, api, allow_loopback)?;
    if !manifest::newer_than(&manifest.version, floor) {
        return Err("Frontend release is not newer than the running frontend".into());
    }
    if !manifest.compatible(api) {
        return Err("Incompatible frontend API".into());
    }
    manifest::validate_url(&manifest.url, allow_loopback)?;
    let archive = download(&client, &manifest.url, MAX_ARCHIVE_BYTES)?;
    let verified = manifest::verify(&metadata, &signature, &archive, key, api, allow_loopback)?;
    store.install(&verified, &signature, &archive)?;
    Ok(verified)
}

fn download(
    client: &reqwest::blocking::Client,
    url: &str,
    limit: usize,
) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let response = client
        .get(url)
        .send()
        .and_then(|response| response.error_for_status())
        .map_err(|e| e.to_string())?;
    if response
        .content_length()
        .is_some_and(|size| size > limit as u64)
    {
        return Err("Download exceeds limit".into());
    }
    let mut bytes = Vec::new();
    response
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > limit {
        return Err("Download exceeds limit".into());
    }
    Ok(bytes)
}

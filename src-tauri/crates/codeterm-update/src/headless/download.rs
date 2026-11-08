use super::{Error, ErrorCode, VerifiedManifest};
use crate::release_transport::{Body, FetchError, Get, ReqwestHttp, Transport};
use codeterm_plugins::update_source::github_release::is_outage_status;
use std::{path::Path, time::Duration};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};

const FEED_TIMEOUT: Duration = Duration::from_secs(60);
const ARCHIVE_TIMEOUT: Duration = Duration::from_secs(600);
const MAX_FEED_BYTES: usize = 64 * 1024;

fn transport_failure(cause: impl ToString) -> Error {
    Error::new(ErrorCode::DownloadFailed, cause)
}

pub async fn fetch() -> Result<VerifiedManifest, Error> {
    let platform = super::platform()?;
    let latest = tokio::time::timeout(
        Duration::from_secs(60),
        crate::checker::resolve_release(&crate::InstallMethod::Unknown, crate::Channel::Stable),
    )
    .await
    .map_err(|_| {
        Error::new(
            ErrorCode::DownloadFailed,
            "Headless release discovery deadline",
        )
    })?
    .map_err(|e| Error::new(ErrorCode::DownloadFailed, e))?;
    let tag = semver::Version::parse(latest.tag.trim_start_matches('v'))
        .map_err(|e| Error::new(ErrorCode::InvalidManifest, e))?;
    let http = ReqwestHttp::new(super::client(FEED_TIMEOUT)?).map_err(transport_failure)?;
    let transport = Transport::official(&http);
    let public_key = super::release_public_key()?;
    let verified = fetch_feed(&transport, latest.repository, &tag, platform, &public_key).await?;
    Ok(verified)
}

pub(super) async fn fetch_feed(
    transport: &Transport<'_>,
    repository: &str,
    tag: &semver::Version,
    platform: &str,
    public_key: &str,
) -> Result<VerifiedManifest, Error> {
    let endpoint = format!(
        "https://github.com/{repository}/releases/download/v{tag}/domios-headless-{platform}.json"
    );
    let fetched = transport
        .fetch_bytes(Get::direct(&endpoint, FEED_TIMEOUT), MAX_FEED_BYTES)
        .await
        .map_err(|error| match error {
            FetchError::TooLarge => {
                Error::new(ErrorCode::InvalidManifest, "Headless feed exceeds 64 KiB")
            }
            FetchError::Failed(cause) => transport_failure(cause),
        })?;
    if fetched.status == 404 {
        return Err(Error::new(
            ErrorCode::SignedHeadlessFeedMissing,
            format!("Missing signed headless feed: {endpoint}"),
        ));
    }
    if !(200..300).contains(&fetched.status) {
        return Err(transport_failure(format!(
            "Headless feed returned HTTP {}",
            fetched.status
        )));
    }
    let verified = super::verify_feed(&fetched.body, public_key, platform)?;
    if verified.manifest().version != tag.to_string() {
        return Err(Error::new(
            ErrorCode::InvalidManifest,
            "Headless feed version differs from release tag",
        ));
    }
    Ok(verified)
}

pub async fn download(offer: &VerifiedManifest, path: &Path) -> Result<(), Error> {
    let http = ReqwestHttp::new(super::client(ARCHIVE_TIMEOUT)?).map_err(transport_failure)?;
    let transport = Transport::official(&http);
    let manifest = offer.manifest();
    download_with(&transport, &manifest.url, manifest.size, path).await?;
    Ok(())
}

enum CopyFailure {
    /// The transfer broke; another route may still serve the file.
    Transfer(String),
    /// The bytes or the disk are wrong; another route cannot help.
    Final(Error),
}

async fn copy_archive(
    body: &mut dyn Body,
    output: &mut tokio::fs::File,
    size: u64,
) -> Result<(), CopyFailure> {
    let mut received = 0u64;
    while let Some(chunk) = body.chunk().await.map_err(CopyFailure::Transfer)? {
        received += chunk.len() as u64;
        if received > size {
            return Err(CopyFailure::Final(Error::new(
                ErrorCode::ArchiveMismatch,
                "Headless download exceeds signed size",
            )));
        }
        output
            .write_all(&chunk)
            .await
            .map_err(|e| CopyFailure::Final(transport_failure(e)))?;
    }
    output
        .sync_all()
        .await
        .map_err(|e| CopyFailure::Final(transport_failure(e)))?;
    if received != size {
        return Err(CopyFailure::Final(Error::new(
            ErrorCode::ArchiveMismatch,
            "Headless download is truncated",
        )));
    }
    Ok(())
}

/// Streams the archive to `path`; `verify_archive` judges the result, whichever route served it.
pub(super) async fn download_with(
    transport: &Transport<'_>,
    url: &str,
    size: u64,
    path: &Path,
) -> Result<(), Error> {
    let mut output = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .await
        .map_err(transport_failure)?;
    let direct = match transport.http.get(Get::direct(url, ARCHIVE_TIMEOUT)).await {
        Ok(reply) if is_outage_status(reply.status) => format!("HTTP {}", reply.status),
        Ok(reply) if !reply.is_success() => {
            return Err(transport_failure(format!(
                "Headless download returned HTTP {}",
                reply.status
            )));
        }
        Ok(mut reply) => match copy_archive(reply.body.as_mut(), &mut output, size).await {
            Ok(()) => return Ok(()),
            Err(CopyFailure::Final(error)) => return Err(error),
            Err(CopyFailure::Transfer(cause)) => cause,
        },
        Err(cause) => cause,
    };
    let mut reply = transport
        .fallback(url, &direct, ARCHIVE_TIMEOUT)
        .await
        .map_err(transport_failure)?;
    output.set_len(0).await.map_err(transport_failure)?;
    output
        .seek(std::io::SeekFrom::Start(0))
        .await
        .map_err(transport_failure)?;
    copy_archive(reply.body.as_mut(), &mut output, size)
        .await
        .map_err(|failure| match failure {
            CopyFailure::Transfer(cause) => transport_failure(
                codeterm_plugins::update_source::github_release::both_failed(&direct, &cause),
            ),
            CopyFailure::Final(error) => error,
        })
}

pub async fn wait_for_health(version: &str, build: &str) -> Result<(), Error> {
    let check = async {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(180))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| e.to_string())?;
        let mut events = client
            .get("http://127.0.0.1:7685/api/startup/events")
            .send()
            .await
            .map_err(|e| e.to_string())?
            .error_for_status()
            .map_err(|e| e.to_string())?;
        let mut pending = Vec::new();
        let mut ready = false;
        while let Some(chunk) = events.chunk().await.map_err(|e| e.to_string())? {
            pending.extend_from_slice(&chunk);
            if pending.len() > 64 * 1024 {
                return Err("Startup event exceeds bound".to_string());
            }
            while let Some(end) = pending.iter().position(|b| *b == b'\n') {
                let line: Vec<_> = pending.drain(..=end).collect();
                if let Some(data) = line
                    .strip_prefix(b"data: ")
                    .or_else(|| line.strip_prefix(b"data:"))
                {
                    let state: serde_json::Value =
                        serde_json::from_slice(data).map_err(|e| e.to_string())?;
                    match state["state"].as_str() {
                        Some("ready") => {
                            ready = true;
                            break;
                        }
                        Some("failed") => return Err("Server startup failed".to_string()),
                        _ => {}
                    }
                }
            }
            if ready {
                break;
            }
        }
        if !ready {
            return Err("Startup stream ended without readiness".to_string());
        }
        let mut response = client
            .get("http://127.0.0.1:7685/api/health")
            .send()
            .await
            .map_err(|e| e.to_string())?
            .error_for_status()
            .map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
            if bytes.len() + chunk.len() > 65536 {
                return Err("Health response exceeds bound".to_string());
            }
            bytes.extend_from_slice(&chunk);
        }
        let health: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        if health["startup"]["state"] != "ready"
            || health["version"] != version
            || health["build_sha"] != build
        {
            return Err("Server health does not match expected version/build".to_string());
        }
        Ok(())
    };
    tokio::time::timeout(Duration::from_secs(180), check)
        .await
        .map_err(|_| Error::new(ErrorCode::HealthFailed, "Server readiness deadline"))?
        .map_err(|e| Error::new(ErrorCode::HealthFailed, e))
}

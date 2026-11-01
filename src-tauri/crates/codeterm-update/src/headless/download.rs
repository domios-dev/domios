use super::{Error, ErrorCode, VerifiedManifest};
use std::{path::Path, time::Duration};
use tokio::io::AsyncWriteExt;

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
    let endpoint = format!(
        "https://github.com/{}/releases/download/v{tag}/domios-headless-{platform}.json",
        latest.repository
    );
    let mut response = super::client(Duration::from_secs(60))?
        .get(&endpoint)
        .send()
        .await
        .map_err(|e| Error::new(ErrorCode::DownloadFailed, e))?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(Error::new(
            ErrorCode::SignedHeadlessFeedMissing,
            format!("Missing signed headless feed: {endpoint}"),
        ));
    }
    response = response
        .error_for_status()
        .map_err(|e| Error::new(ErrorCode::DownloadFailed, e))?;
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| Error::new(ErrorCode::DownloadFailed, e))?
    {
        if bytes.len() + chunk.len() > 64 * 1024 {
            return Err(Error::new(
                ErrorCode::InvalidManifest,
                "Headless feed exceeds 64 KiB",
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    let verified = super::verify_feed(&bytes, &super::release_public_key()?, platform)?;
    if verified.manifest().version != tag.to_string() {
        return Err(Error::new(
            ErrorCode::InvalidManifest,
            "Headless feed version differs from release tag",
        ));
    }
    Ok(verified)
}

pub async fn download(offer: &VerifiedManifest, path: &Path) -> Result<(), Error> {
    let mut response = super::client(Duration::from_secs(600))?
        .get(&offer.manifest().url)
        .send()
        .await
        .map_err(|e| Error::new(ErrorCode::DownloadFailed, e))?
        .error_for_status()
        .map_err(|e| Error::new(ErrorCode::DownloadFailed, e))?;
    let mut output = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .await
        .map_err(|e| Error::new(ErrorCode::DownloadFailed, e))?;
    let mut received = 0u64;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| Error::new(ErrorCode::DownloadFailed, e))?
    {
        received += chunk.len() as u64;
        if received > offer.manifest().size {
            return Err(Error::new(
                ErrorCode::ArchiveMismatch,
                "Headless download exceeds signed size",
            ));
        }
        output
            .write_all(&chunk)
            .await
            .map_err(|e| Error::new(ErrorCode::DownloadFailed, e))?;
    }
    output
        .sync_all()
        .await
        .map_err(|e| Error::new(ErrorCode::DownloadFailed, e))?;
    if received != offer.manifest().size {
        return Err(Error::new(
            ErrorCode::ArchiveMismatch,
            "Headless download is truncated",
        ));
    }
    Ok(())
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

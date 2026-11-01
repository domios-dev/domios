use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{io::Read, path::Path, sync::Arc, time::Duration};

mod download;
pub mod transaction;
pub use download::{download, fetch, wait_for_health};

pub const MAX_ARCHIVE_BYTES: u64 = 1024 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    ContainerManagedUpdate,
    SignedHeadlessFeedMissing,
    InvalidSignature,
    InvalidManifest,
    DowngradeRefused,
    DownloadFailed,
    ArchiveMismatch,
    StageFailed,
    SwapFailed,
    RestartFailed,
    HealthFailed,
    RollbackFailed,
    HelperUnavailable,
    HelperInterrupted,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Error {
    pub code: ErrorCode,
    pub message: String,
}

impl Error {
    pub fn new(code: ErrorCode, message: impl ToString) -> Self {
        Self {
            code,
            message: message.to_string(),
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.code, self.message)
    }
}

impl std::error::Error for Error {}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SignedFeed {
    manifest: String,
    signature: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: u32,
    pub platform: String,
    pub version: String,
    pub archive: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
    pub allow_downgrade: bool,
}

#[derive(Clone, Debug)]
pub struct VerifiedManifest(Manifest);

impl VerifiedManifest {
    pub fn manifest(&self) -> &Manifest {
        &self.0
    }

    pub fn target_version(&self, installed: &str) -> Result<Option<&str>, Error> {
        let parse =
            |v| semver::Version::parse(v).map_err(|e| Error::new(ErrorCode::InvalidManifest, e));
        let target = parse(&self.0.version)?;
        let installed = parse(installed)?;
        match target.cmp_precedence(&installed) {
            std::cmp::Ordering::Equal => Ok(None),
            std::cmp::Ordering::Less if !self.0.allow_downgrade => Err(Error::new(
                ErrorCode::DowngradeRefused,
                "Signed headless manifest does not authorize a downgrade",
            )),
            _ => Ok(Some(&self.0.version)),
        }
    }

    pub fn verify_archive(&self, reader: impl Read) -> Result<(), Error> {
        let mut reader = reader.take(self.0.size + 1);
        let mut digest = Sha256::new();
        let mut size = 0u64;
        let mut buffer = [0u8; 65536];
        loop {
            let count = reader
                .read(&mut buffer)
                .map_err(|e| Error::new(ErrorCode::DownloadFailed, e))?;
            if count == 0 {
                break;
            }
            size += count as u64;
            digest.update(&buffer[..count]);
        }
        if size != self.0.size || format!("{:x}", digest.finalize()) != self.0.sha256 {
            return Err(Error::new(
                ErrorCode::ArchiveMismatch,
                "Headless archive size or SHA-256 differs from signed manifest",
            ));
        }
        Ok(())
    }
}

pub fn release_public_key() -> Result<String, Error> {
    let config: serde_json::Value = serde_json::from_str(include_str!("../../../tauri.conf.json"))
        .map_err(|e| Error::new(ErrorCode::InvalidManifest, e))?;
    config["plugins"]["updater"]["pubkey"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| {
            Error::new(
                ErrorCode::InvalidManifest,
                "Missing embedded release public key",
            )
        })
}

pub fn verify_feed(
    bytes: &[u8],
    public_key: &str,
    platform: &str,
) -> Result<VerifiedManifest, Error> {
    let envelope: SignedFeed =
        serde_json::from_slice(bytes).map_err(|e| Error::new(ErrorCode::InvalidSignature, e))?;
    let decode = |text: &str| {
        STANDARD
            .decode(text)
            .map_err(|e| Error::new(ErrorCode::InvalidSignature, e))
    };
    let decode_text = |text: &str| {
        String::from_utf8(decode(text)?).map_err(|e| Error::new(ErrorCode::InvalidSignature, e))
    };
    let manifest_bytes = decode(&envelope.manifest)?;
    let key = minisign_verify::PublicKey::decode(&decode_text(public_key)?)
        .map_err(|e| Error::new(ErrorCode::InvalidSignature, e))?;
    let signature = minisign_verify::Signature::decode(&decode_text(&envelope.signature)?)
        .map_err(|e| Error::new(ErrorCode::InvalidSignature, e))?;
    key.verify(&manifest_bytes, &signature, true)
        .map_err(|e| Error::new(ErrorCode::InvalidSignature, e))?;
    let manifest: Manifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|e| Error::new(ErrorCode::InvalidManifest, e))?;
    let expected_name = format!("domios-headless-v{}-{}.tar.gz", manifest.version, platform);
    let url = reqwest::Url::parse(&manifest.url)
        .map_err(|e| Error::new(ErrorCode::InvalidManifest, e))?;
    if manifest.schema != 1
        || manifest.platform != platform
        || manifest.archive != expected_name
        || !matches!(platform, "linux-x86_64" | "linux-aarch64")
        || semver::Version::parse(&manifest.version).is_err()
        || manifest.sha256.len() != 64
        || !manifest
            .sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || manifest.size == 0
        || manifest.size > MAX_ARCHIVE_BYTES
        || url.scheme() != "https"
        || url.host_str().is_none()
        || url.path_segments().and_then(Iterator::last) != Some(manifest.archive.as_str())
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(Error::new(
            ErrorCode::InvalidManifest,
            "Invalid signed headless archive identity",
        ));
    }
    Ok(VerifiedManifest(manifest))
}

#[async_trait::async_trait]
pub trait Backend: Send + Sync {
    async fn check(&self) -> Result<crate::update_state::UpdateInfo, Error>;
    async fn install(&self) -> Result<serde_json::Value, Error>;
    fn status(&self) -> Option<serde_json::Value>;
}

static BACKEND: std::sync::OnceLock<Arc<dyn Backend>> = std::sync::OnceLock::new();

pub fn register(backend: Arc<dyn Backend>) -> Result<(), Arc<dyn Backend>> {
    BACKEND.set(backend)
}
pub fn backend() -> Option<&'static Arc<dyn Backend>> {
    BACKEND.get()
}

pub fn platform() -> Result<&'static str, Error> {
    match crate::manifest::current_platform_key() {
        Some(key @ ("linux-x86_64" | "linux-aarch64")) => Ok(key),
        _ => Err(Error::new(
            ErrorCode::HelperUnavailable,
            "Headless self-update requires Linux",
        )),
    }
}

pub fn archive_file(path: &Path) -> Result<std::fs::File, Error> {
    std::fs::File::open(path).map_err(|e| Error::new(ErrorCode::DownloadFailed, e))
}

pub(super) fn client(timeout: Duration) -> Result<reqwest::Client, Error> {
    reqwest::Client::builder()
        .https_only(true)
        .timeout(timeout)
        .user_agent("Domios-Headless-Updater")
        .build()
        .map_err(|e| Error::new(ErrorCode::DownloadFailed, e))
}

#[cfg(test)]
mod tests;

use base64::Engine;
use std::path::{Path, PathBuf};

mod staging;
pub use staging::{cleanup, failure, StageLease};
#[cfg(any(test, target_os = "linux"))]
pub mod handoff;

pub struct StagedDebianPackage {
    pub path: PathBuf,
    pub version: String,
    pub sha256: String,
    command: String,
    pub lease: StageLease,
}

impl StagedDebianPackage {
    pub fn command(&self) -> String {
        self.command.clone()
    }

    pub fn response(&self) -> serde_json::Value {
        serde_json::json!({
            "status": "elevation_required", "install_method": "deb",
            "command": self.command(), "staged_path": self.path,
            "message": "The Debian package has been downloaded and its signature verified. Installation requires root. In a terminal outside Domios, quit Domios, confirm it has stopped, then run the supplied dpkg command and relaunch Domios. Do not run the quit step inside a Domios tab."
        })
    }
}

pub async fn stage(manifest_url: Option<&str>) -> Result<Option<StagedDebianPackage>, String> {
    let manifest = crate::checker::resolve_manifest(
        &crate::InstallMethod::Deb,
        crate::Channel::Stable,
        manifest_url,
    )
    .await?;
    if crate::manifest::offered_version(
        codeterm_build_info::version(),
        &manifest,
        crate::manifest::current_platform_key(),
    )?
    .is_none()
    {
        return Ok(None);
    }
    let platform = crate::manifest::current_platform_key()
        .and_then(|key| manifest.platforms.get(key))
        .ok_or("Missing Debian artifact")?;
    crate::manifest::validate_url(&platform.url)?;
    let url = reqwest::Url::parse(&platform.url).map_err(|error| error.to_string())?;
    if !url.path().ends_with(".deb") {
        return Err("Debian updater refused non-.deb payload".into());
    }
    let config: serde_json::Value = serde_json::from_str(include_str!("../../../tauri.conf.json"))
        .map_err(|error| error.to_string())?;
    let public_key = config["plugins"]["updater"]["pubkey"]
        .as_str()
        .ok_or("Missing updater public key")?
        .to_owned();
    let signature = platform.signature.clone();
    let version = manifest.version.clone();
    let reuse_signature = signature.clone();
    let reuse_key = public_key.clone();
    let reuse_version = version.clone();
    if let Some(staged) = tokio::task::spawn_blocking(move || {
        staging::reuse(
            &std::env::temp_dir(),
            &reuse_version,
            &reuse_signature,
            &reuse_key,
        )
    })
    .await
    .map_err(|error| error.to_string())??
    {
        return Ok(Some(staged));
    }
    let bytes = reqwest::Client::builder()
        .https_only(!cfg!(feature = "dev-updater-allowlocal"))
        .build()
        .map_err(|error| error.to_string())?
        .get(url)
        .send()
        .await
        .map_err(|error| error.to_string())?
        .error_for_status()
        .map_err(|error| error.to_string())?
        .bytes()
        .await
        .map_err(|error| error.to_string())?;
    tokio::task::spawn_blocking(move || {
        verify_and_stage(
            &bytes,
            &signature,
            &public_key,
            &version,
            &std::env::temp_dir(),
        )
    })
    .await
    .map_err(|error| error.to_string())?
    .map(Some)
}

fn verify_and_stage(
    bytes: &[u8],
    signature: &str,
    public_key: &str,
    version: &str,
    root: &Path,
) -> Result<StagedDebianPackage, String> {
    verify(bytes, signature, public_key)?;
    staging::store(bytes, version, signature, root)
}

fn verify(bytes: &[u8], signature: &str, public_key: &str) -> Result<(), String> {
    let decode = |value: &str| -> Result<String, String> {
        String::from_utf8(
            base64::engine::general_purpose::STANDARD
                .decode(value)
                .map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())
    };
    let key = minisign_verify::PublicKey::decode(&decode(public_key)?)
        .map_err(|error| error.to_string())?;
    let signature = minisign_verify::Signature::decode(&decode(signature)?)
        .map_err(|error| error.to_string())?;
    key.verify(bytes, &signature, true)
        .map_err(|error| format!("Debian package signature verification failed: {error}"))
}

fn staged_command(path: &Path) -> Result<String, String> {
    let path = path
        .to_str()
        .ok_or("staged Debian package path is not UTF-8")?;
    Ok(format!("sudo dpkg -i -- '{}'", path.replace('\'', "'\\''")))
}

#[cfg(test)]
mod tests;

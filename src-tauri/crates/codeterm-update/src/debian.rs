use base64::Engine;
use std::path::{Path, PathBuf};

pub struct StagedDebianPackage {
    path: PathBuf,
    command: String,
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
        verify_and_stage(&bytes, &signature, &public_key, &std::env::temp_dir())
    })
    .await
    .map_err(|error| error.to_string())?
    .map(Some)
}

fn verify_and_stage(
    bytes: &[u8],
    signature: &str,
    public_key: &str,
    root: &Path,
) -> Result<StagedDebianPackage, String> {
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
        .map_err(|error| format!("Debian package signature verification failed: {error}"))?;
    let directory = tempfile::Builder::new()
        .prefix("domios-verified-update-")
        .tempdir_in(root)
        .map_err(|error| error.to_string())?;
    let path = directory.path().join("domios-update.deb");
    let command = staged_command(&path)?;
    std::fs::write(&path, bytes).map_err(|error| error.to_string())?;
    let _ = directory.keep();
    Ok(StagedDebianPackage { path, command })
}

fn staged_command(path: &Path) -> Result<String, String> {
    let path = path
        .to_str()
        .ok_or("staged Debian package path is not UTF-8")?;
    Ok(format!("sudo dpkg -i -- '{}'", path.replace('\'', "'\\''")))
}

#[cfg(test)]
mod tests;

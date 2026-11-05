use super::{exact_package, verify_download, MAX_ARCHIVE_BYTES};
use base64::Engine;
use serde::Deserialize;
use sha2::{Digest, Sha512};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

#[derive(Deserialize)]
struct Manifest {
    name: String,
    version: String,
    bin: BTreeMap<String, String>,
    dist: Distribution,
}
#[derive(Deserialize)]
struct Distribution {
    tarball: String,
    integrity: String,
    #[serde(rename = "unpackedSize")]
    unpacked_size: Option<u64>,
}
#[async_trait::async_trait]
trait InstallPort: Sync {
    async fn download(&self, url: &str, max: usize) -> Result<Vec<u8>, String>;
    async fn package_manager(&self, target: &Path, archive: &Path) -> Result<PathBuf, String>;
}
struct NativePort {
    client: reqwest::Client,
}
#[async_trait::async_trait]
impl InstallPort for NativePort {
    async fn download(&self, url: &str, max: usize) -> Result<Vec<u8>, String> {
        download(&self.client, url, max).await
    }
    async fn package_manager(&self, target: &Path, archive: &Path) -> Result<PathBuf, String> {
        package_manager(target, archive).await
    }
}
async fn download(client: &reqwest::Client, url: &str, max: usize) -> Result<Vec<u8>, String> {
    verify_download(url, &[], None)?;
    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|_| "Package download failed")?
        .error_for_status()
        .map_err(|_| "Package download failed")?;
    if response
        .content_length()
        .is_some_and(|len| len > max as u64)
    {
        return Err("Oversized package download".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "Package download failed")?
    {
        if chunk.len() > max.saturating_sub(bytes.len()) {
            return Err("Oversized package download".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
fn validate_manifest(bytes: &[u8], package: &str, version: &str) -> Result<Manifest, String> {
    let manifest: Manifest =
        serde_json::from_slice(bytes).map_err(|_| "Malformed package manifest")?;
    if manifest.name != package
        || manifest.version != version
        || manifest.bin.len() != 1
        || manifest
            .unpacked_size()
            .is_none_or(|size| size > MAX_ARCHIVE_BYTES as u64)
        || !manifest.dist.integrity.starts_with("sha512-")
        || manifest.bin.iter().any(|(name, path)| {
            name.is_empty()
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
                || path.starts_with('/')
                || path.contains('\\')
                || path.split('/').any(|part| part == "..")
        })
    {
        return Err("Malformed package manifest".into());
    }
    Ok(manifest)
}
impl Manifest {
    fn unpacked_size(&self) -> Option<u64> {
        self.dist.unpacked_size
    }
}
pub(super) async fn install(
    target: &Path,
    package: &str,
    version: &str,
) -> Result<(codeterm_plugin::agent_runtime::AgentRuntimeCommand, String), String> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|_| "Package downloader unavailable")?;
    install_using(target, package, version, &NativePort { client }).await
}
async fn install_using(
    target: &Path,
    package: &str,
    version: &str,
    port: &dyn InstallPort,
) -> Result<(codeterm_plugin::agent_runtime::AgentRuntimeCommand, String), String> {
    exact_package(&format!("{package}@{version}"))?;
    let url = format!(
        "https://registry.npmjs.org/{}/{version}",
        package.replace('/', "%2f")
    );
    verify_download(&url, &[], None)?;
    let metadata = port.download(&url, 1024 * 1024).await?;
    if metadata.len() > 1024 * 1024 {
        return Err("Oversized package manifest".into());
    }
    let manifest = validate_manifest(&metadata, package, version)?;
    verify_download(&manifest.dist.tarball, &[], None)?;
    let bytes = port
        .download(&manifest.dist.tarball, MAX_ARCHIVE_BYTES)
        .await?;
    let sha256 = verify_download(&manifest.dist.tarball, &bytes, None)?;
    let expected = base64::prelude::BASE64_STANDARD
        .decode(
            manifest
                .dist
                .integrity
                .strip_prefix("sha512-")
                .ok_or("Package integrity unavailable")?,
        )
        .map_err(|_| "Malformed package integrity")?;
    if expected.as_slice() != &Sha512::digest(&bytes)[..] {
        return Err("Agent archive checksum mismatch".into());
    }
    tokio::fs::create_dir_all(target)
        .await
        .map_err(|_| "Install directory unavailable")?;
    let archive = target.join("agent.tgz");
    tokio::fs::write(&archive, &bytes)
        .await
        .map_err(|_| "Package archive could not be saved")?;
    let node = port.package_manager(target, &archive).await?;
    let relative = manifest
        .bin
        .values()
        .next()
        .ok_or("Package binary unavailable")?;
    let binary = target.join("node_modules").join(package).join(relative);
    if !binary.is_file() {
        return Err("Installed package binary unavailable".into());
    }
    let binary = binary
        .canonicalize()
        .map_err(|_| "Installed binary path unavailable")?;
    if !binary.starts_with(
        target
            .canonicalize()
            .map_err(|_| "Install path unavailable")?,
    ) {
        return Err("Package binary escapes install directory".into());
    }
    Ok((
        codeterm_plugin::agent_runtime::AgentRuntimeCommand {
            binary: node.to_string_lossy().into_owned(),
            args: vec![binary.to_string_lossy().into_owned()],
        },
        sha256,
    ))
}

async fn package_manager(target: &Path, archive: &Path) -> Result<PathBuf, String> {
    let runner = super::resolve_binary("npm")?.ok_or("npm is not installed")?;
    let runner = runner.canonicalize().map_err(|_| "npm path unavailable")?;
    let cli = if runner.extension().is_some_and(|ext| ext == "cmd") {
        runner
            .parent()
            .ok_or("npm directory unavailable")?
            .join("node_modules/npm/bin/npm-cli.js")
    } else {
        runner
    };
    if !cli.is_file() || cli.extension().is_none_or(|ext| ext != "js") {
        return Err("A native npm CLI entry point is required".into());
    }
    let node = super::resolve_binary("node")?.ok_or("Node.js is not installed")?;
    let mut command = tokio::process::Command::new(&node);
    crate::process_util::no_window_tokio(&mut command);
    command.arg(cli);
    command.env_clear();
    for name in [
        "PATH",
        "HOME",
        "USERPROFILE",
        "SystemRoot",
        "TEMP",
        "TMP",
        "TMPDIR",
    ] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    command
        .args([
            "install",
            "--ignore-scripts",
            "--no-audit",
            "--no-fund",
            "--no-bin-links",
            "--prefix",
        ])
        .arg(target)
        .arg(archive)
        .current_dir(target)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let status = tokio::time::timeout(Duration::from_secs(120), command.status())
        .await
        .map_err(|_| "Package install deadline exceeded")?
        .map_err(|_| "Package installer failed")?;
    if !status.success() {
        return Err("Package installer failed".into());
    }
    Ok(node)
}

#[cfg(test)]
mod tests;

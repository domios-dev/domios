use codeterm_plugin::agent_runtime::{AgentRuntimeCommand, RuntimeLaunchProfile};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

mod binding;
mod distribution;
mod npm;
#[cfg(test)]
mod tests;

pub(super) use binding::{bound_by, fingerprint, Binding};

pub const MAX_ARCHIVE_BYTES: usize = 64 * 1024 * 1024;
const MAX_RECEIPT_BYTES: u64 = 64 * 1024;
static RECEIPT_WRITES: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstallReceipt {
    pub profile_id: String,
    pub declaration_sha256: String,
    pub package: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    pub command: AgentRuntimeCommand,
    pub timestamp_ms: u64,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InstallState {
    NotInstalled,
    Installed,
    Detected,
    Unknown,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileState {
    pub id: String,
    pub config_options: Vec<codeterm_plugin::agent_runtime::RuntimeConfigOption>,
    pub install: InstallState,
    pub auth: &'static str,
    pub can_install: bool,
    pub owner_configured: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub setup: Option<super::launch_profile_setup::SetupJob>,
}

pub fn profile_root(provider: &str) -> Result<PathBuf, String> {
    valid_id(provider)?;
    Ok(crate::paths::data_dir_path()
        .join("providers")
        .join(provider)
        .join("profiles"))
}
fn valid_id(id: &str) -> Result<(), String> {
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
    {
        return Err("Invalid profile identifier".into());
    }
    Ok(())
}
fn receipt_path(root: &Path, id: &str) -> Result<PathBuf, String> {
    valid_id(id)?;
    Ok(root.join(format!("{id}.json")))
}
pub fn read_receipt(
    root: &Path,
    id: &str,
    profile: &RuntimeLaunchProfile,
) -> Result<Option<InstallReceipt>, String> {
    let path = receipt_path(root, id)?;
    let file = match std::fs::File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("Install receipt unavailable".into()),
    };
    if file
        .metadata()
        .map_err(|_| "Install receipt unavailable")?
        .len()
        > MAX_RECEIPT_BYTES
    {
        return Err("Oversized install receipt".into());
    }
    use std::io::Read;
    let mut bytes = Vec::new();
    file.take(MAX_RECEIPT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Install receipt unavailable")?;
    if bytes.len() as u64 > MAX_RECEIPT_BYTES {
        return Err("Oversized install receipt".into());
    }
    let receipt: InstallReceipt =
        serde_json::from_slice(&bytes).map_err(|_| "Malformed install receipt")?;
    if receipt.profile_id != id {
        return Ok(None);
    }
    let Some(bound) = bound_by(profile, &receipt.declaration_sha256)? else {
        return Ok(None);
    };
    validate_command(&receipt.command)?;
    if !Path::new(&receipt.command.binary).is_absolute()
        || !Path::new(&receipt.command.binary).is_file()
    {
        return Ok(None);
    }
    if profile
        .install
        .as_ref()
        .is_some_and(|spec| spec.distribution.as_deref().unwrap_or("npx") == "npx")
        && !receipt
            .command
            .args
            .first()
            .is_some_and(|script| Path::new(script).is_absolute() && Path::new(script).is_file())
    {
        return Ok(None);
    }
    if profile
        .install
        .as_ref()
        .is_some_and(|spec| spec.distribution.as_deref() == Some("binary"))
    {
        let bytes = distribution::read_executable(Path::new(&receipt.command.binary))?;
        if receipt.sha256.as_deref() != Some(format!("{:x}", Sha256::digest(&bytes)).as_str()) {
            return Err("Installed binary checksum mismatch".into());
        }
    }
    if bound == Binding::LegacyDeclaration {
        return Ok(Some(rebind(&path, &bytes, receipt, profile)?));
    }
    Ok(Some(receipt))
}
/// A verified legacy receipt is rebound once to its install identity, so later declaration
/// fields cannot stale it; a receipt rewritten meanwhile is left as it is.
fn rebind(
    path: &Path,
    verified: &[u8],
    mut receipt: InstallReceipt,
    profile: &RuntimeLaunchProfile,
) -> Result<InstallReceipt, String> {
    receipt.declaration_sha256 = fingerprint(profile)?;
    let bytes = serde_json::to_vec(&receipt).map_err(|_| "Invalid receipt")?;
    let _writes = RECEIPT_WRITES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if std::fs::read(path).ok().as_deref() == Some(verified)
        && codeterm_util::atomic_write::write_atomic(path, bytes).is_err()
    {
        log::debug!("[launch-profile] legacy install receipt kept: rebinding could not be saved");
    }
    Ok(receipt)
}
fn write_receipt(root: &Path, receipt: &InstallReceipt) -> Result<(), String> {
    std::fs::create_dir_all(root).map_err(|_| "Profile data directory unavailable")?;
    let path = receipt_path(root, &receipt.profile_id)?;
    let bytes = serde_json::to_vec(receipt).map_err(|_| "Invalid receipt")?;
    {
        let _writes = RECEIPT_WRITES
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        codeterm_util::atomic_write::write_atomic(&path, bytes)
            .map_err(|_| "Install receipt could not be saved".to_string())?;
    }
    crate::provider::invalidate_profile_readiness(root);
    Ok(())
}
pub(super) fn remove_file_if_present(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
            Err(format!("Could not delete {}: {error}", path.display()))
        }
        _ => Ok(()),
    }
}
/// Deletes what Domios keeps for exactly this profile: its `<id>/` tree of installs and process
/// folders, its options and its receipt.
pub(super) fn remove_files(root: &Path, id: &str) -> Result<(), String> {
    let receipt = receipt_path(root, id)?;
    let directory = root.join(id);
    match std::fs::remove_dir_all(&directory) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
            return Err(format!("Could not delete {}: {error}", directory.display()));
        }
        _ => {}
    }
    super::profile_options::forget(root, id)?;
    if codeterm_plugin::agent_runtime::owner_profiles::names_the_store(id) {
        return Ok(());
    }
    let _writes = RECEIPT_WRITES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    remove_file_if_present(&receipt)
}
fn validate_command(command: &AgentRuntimeCommand) -> Result<(), String> {
    let capability: codeterm_plugin::agent_runtime::AgentRuntimeCapability = serde_json::from_value(serde_json::json!({
        "runtime":"stdio-jsonrpc", "command":command, "requestMethods":{"initialize":["initialize"]}, "notificationMethods":[]
    })).map_err(|_| "Invalid local command")?;
    capability
        .validate_declaration()
        .map_err(|_| "Invalid local command")?;
    let basename = Path::new(&command.binary)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ["npx", "npm", "uvx", "uv", "env"].contains(&basename.as_str()) {
        return Err("Local commands must name the installed agent binary".into());
    }
    Ok(())
}
fn resolve_binary(binary: &str) -> Result<Option<PathBuf>, String> {
    match codeterm_plugin::js_runtime::probe_bin(binary).resolution {
        codeterm_plugin::js_runtime::BinResolution::Found(path) => Ok(Some(path)),
        codeterm_plugin::js_runtime::BinResolution::Missing => Ok(None),
        codeterm_plugin::js_runtime::BinResolution::Unknown => {
            Err("Binary availability unknown".into())
        }
    }
}
pub fn detect(root: &Path, id: &str, profile: &RuntimeLaunchProfile) -> InstallState {
    match read_receipt(root, id, profile) {
        Ok(Some(_)) => InstallState::Installed,
        Err(_) => InstallState::Unknown,
        Ok(None) if profile.install.is_some() || profile.owner_configured => {
            InstallState::NotInstalled
        }
        Ok(None) => match resolve_binary(&profile.binary) {
            Ok(Some(_)) => InstallState::Detected,
            Ok(None) => InstallState::NotInstalled,
            Err(_) => InstallState::Unknown,
        },
    }
}
fn receipt(
    root: &Path,
    id: &str,
    profile: &RuntimeLaunchProfile,
    package: String,
    version: String,
    sha256: Option<String>,
    command: AgentRuntimeCommand,
) -> Result<InstallReceipt, String> {
    let receipt = InstallReceipt {
        profile_id: id.into(),
        declaration_sha256: fingerprint(profile)?,
        package,
        version,
        sha256,
        command,
        timestamp_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| "Clock unavailable")?
            .as_millis() as u64,
    };
    write_receipt(root, &receipt)?;
    Ok(receipt)
}
pub fn confirm_local(
    root: &Path,
    id: &str,
    profile: &RuntimeLaunchProfile,
    command: AgentRuntimeCommand,
    confirmed: bool,
) -> Result<InstallReceipt, String> {
    if !confirmed || !profile.owner_configured {
        return Err("Owner confirmation of the local command is required".into());
    }
    validate_command(&command)?;
    let binary = resolve_binary(&command.binary)?.ok_or("Local agent binary not found")?;
    let mut command = command;
    command.binary = binary.to_string_lossy().into_owned();
    receipt(
        root,
        id,
        profile,
        command.binary.clone(),
        "local".into(),
        None,
        command,
    )
}
pub async fn install(
    root: &Path,
    id: &str,
    profile: &RuntimeLaunchProfile,
    confirmed: bool,
) -> Result<InstallReceipt, String> {
    if !confirmed {
        return Err("Explicit install confirmation is required".into());
    }
    valid_id(id)?;
    let spec = profile
        .install
        .as_ref()
        .ok_or("Profile has no supported installer")?;
    if !codeterm_plugin::agent_runtime::install_validation::validate(profile) {
        return Err("Invalid pinned install declaration".into());
    }
    let kind = spec.distribution.as_deref().unwrap_or("npx");
    let (package, version) = if kind == "uvx" {
        codeterm_plugin::agent_runtime::install_validation::exact_python(&spec.package)
            .ok_or("An exact Python package version is required")?
    } else {
        exact_package(&spec.package)?
    };
    if spec
        .registry
        .as_deref()
        .is_some_and(|url| url != "https://registry.npmjs.org")
    {
        return Err("Invalid package registry URL".into());
    }
    let target = root.join(id).join(uuid::Uuid::new_v4().to_string());
    let (mut command, sha256) = match kind {
        "npx" => npm::install(&target, package, version).await?,
        "uvx" => distribution::python(&target, &spec.package).await?,
        "binary" => distribution::binary(&target, spec).await?,
        _ => return Err("Unsupported install distribution".into()),
    };
    command.args.extend(
        profile
            .args
            .iter()
            .skip(usize::from(kind != "binary"))
            .cloned(),
    );
    receipt(
        root,
        id,
        profile,
        spec.package.clone(),
        version.into(),
        Some(sha256),
        command,
    )
}
pub(super) fn exact_package(spec: &str) -> Result<(&str, &str), String> {
    let (package, version) = spec
        .rsplit_once('@')
        .ok_or("An exact package version is required")?;
    let name = package.strip_prefix('@').unwrap_or(package);
    if name.is_empty()
        || name.len() > 214
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-/".contains(&b))
        || name
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
        || name.matches('/').count() > usize::from(package.starts_with('@'))
        || semver::Version::parse(version).is_err()
    {
        return Err("An exact package version is required".into());
    }
    Ok((package, version))
}
pub(super) fn verify_download(
    url: &str,
    bytes: &[u8],
    expected: Option<&str>,
) -> Result<String, String> {
    let url = url::Url::parse(url).map_err(|_| "Invalid download URL")?;
    if url.scheme() != "https"
        || url.host_str() != Some("registry.npmjs.org")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.fragment().is_some()
    {
        return Err("Invalid download URL".into());
    }
    if bytes.len() > MAX_ARCHIVE_BYTES {
        return Err("Oversized agent archive".into());
    }
    let sha256 = format!("{:x}", Sha256::digest(bytes));
    if expected.is_some_and(|expected| !expected.eq_ignore_ascii_case(&sha256)) {
        return Err("Agent archive checksum mismatch".into());
    }
    Ok(sha256)
}
pub fn catalog(
    svc: &crate::service::ServiceContext,
    provider: &str,
) -> Result<Vec<ProfileState>, String> {
    let capability = svc
        .agent_registry
        .with_provider(provider, |p| p.agent_runtime())
        .flatten()
        .ok_or("Runtime provider unavailable")?;
    let profiles = capability
        .launch_profiles
        .ok_or("Provider has no launch profiles")?;
    let root = profile_root(provider)?;
    let mut states: Vec<_> = profiles
        .iter()
        .map(|(id, profile)| ProfileState {
            id: id.clone(),
            config_options: super::profile_options::read(&root, id, profile),
            install: detect(&root, id, profile),
            auth: "unknown",
            can_install: profile.install.is_some(),
            owner_configured: profile.owner_configured,
            setup: super::launch_profile_setup::current(provider, id),
        })
        .collect();
    for job in super::launch_profile_setup::for_provider(provider) {
        if !profiles.contains_key(&job.profile_id) {
            states.push(ProfileState {
                id: job.profile_id.clone(),
                config_options: Vec::new(),
                install: InstallState::NotInstalled,
                auth: "unknown",
                can_install: false,
                owner_configured: false,
                setup: Some(job),
            });
        }
    }
    states.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(states)
}
pub fn declaration(
    svc: &crate::service::ServiceContext,
    provider: &str,
    id: &str,
) -> Result<RuntimeLaunchProfile, String> {
    svc.agent_registry
        .with_provider(provider, |p| p.agent_runtime())
        .flatten()
        .and_then(|cap| cap.launch_profiles?.get(id).cloned())
        .ok_or("Unknown launch profile".into())
}
pub(super) fn compose_secret_environment(
    profile: &RuntimeLaunchProfile,
    mut get: impl FnMut(&str) -> Option<String>,
) -> Result<BTreeMap<String, String>, super::model::RuntimeFailure> {
    profile
        .secret_env
        .iter()
        .map(|(env, key)| {
            let value = get(key)
                .filter(|value| {
                    !value.trim().is_empty() && value.len() <= 16384 && !value.contains('\0')
                })
                .ok_or(super::model::RuntimeFailure::SignInRequired)?;
            Ok((env.clone(), value))
        })
        .collect()
}
pub(super) fn secret_environment(
    svc: &crate::service::ServiceContext,
    provider: &str,
    profile: &RuntimeLaunchProfile,
) -> Result<BTreeMap<String, String>, super::model::RuntimeFailure> {
    let prefix = format!("domios:provider:{provider}:");
    if profile
        .secret_env
        .values()
        .any(|key| !key.starts_with(&prefix))
    {
        return Err(super::model::RuntimeFailure::Refused(
            "Profile secret reference is outside its provider namespace".into(),
        ));
    }
    compose_secret_environment(profile, |key| {
        crate::async_runtime::block_on(async {
            let store = svc.secrets.read().await.clone();
            tokio::time::timeout(std::time::Duration::from_secs(10), store.get(key))
                .await
                .ok()?
                .ok()
                .map(|secret| secret.value.into_inner())
        })
    })
}

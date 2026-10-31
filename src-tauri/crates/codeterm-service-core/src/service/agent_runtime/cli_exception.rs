use super::model::RuntimeFailure;
use crate::service::ServiceContext;
use codeterm_plugin::agent_runtime::{
    RestrictedCliCapability, RestrictedCliPolicy, RuntimeContext,
};
use codeterm_types::plan::{ExecutionIntent, ExecutionSandboxPolicy};
use parking_lot::Mutex;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};

const CONFIG: &[u8] = b"# Host-managed restricted CLI exception.\n";
const RULE: &str = "domios-cli.rules";
pub(super) const SCOPE_ENV: &str = "CODETERM_CLI_SCOPE";
pub(super) const SCOPE: &str = "service-operations";
const DENIED: &str = "Restricted CLI exception identity or protected policy changed";

#[derive(Clone)]
struct Lease {
    policy: RestrictedCliPolicy,
    sandbox: ExecutionSandboxPolicy,
    provider: String,
    _executable_pin: Arc<File>,
}

static LEASES: LazyLock<Mutex<HashMap<String, Lease>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn record_policy(policy: &RestrictedCliPolicy) {
    log::debug!("[runtime-policy] phase=cli_exception tab={} epoch={} scope=service-operations executable_pinned=true executable_sha256={} config_sha256={} rule_sha256={}",
        policy.tab_id, policy.epoch, policy.executable_sha256, policy.config_sha256, policy.rule_sha256);
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn file_digest(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|_| DENIED)?;
    if file.metadata().map_err(|_| DENIED)?.len() > 128 * 1024 * 1024 {
        return Err(DENIED.into());
    }
    let mut digest = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let size = file.read(&mut buffer).map_err(|_| DENIED)?;
        if size == 0 {
            break;
        }
        digest.update(&buffer[..size]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn pin_executable(path: &Path) -> Result<Arc<File>, String> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ);
    }
    options.open(path).map(Arc::new).map_err(|_| DENIED.into())
}

fn plain_path(path: &Path) -> Result<String, String> {
    path.to_str()
        .map(|path| codeterm_paths::strip_verbatim_prefix(path).replace('\\', "/"))
        .ok_or_else(|| DENIED.into())
}

fn literal_command_path(path: &str) -> bool {
    !path.is_empty() && path.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"/\\:._-".contains(&byte))
}

fn command_path(path: &Path) -> Result<String, String> {
    let plain = plain_path(path)?;
    if literal_command_path(&plain) { return Ok(plain); }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let input: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut short = vec![0u16; 32768];
        // SAFETY: input is NUL-terminated and output has the supplied UTF-16 capacity.
        let size = unsafe { windows_sys::Win32::Storage::FileSystem::GetShortPathNameW(input.as_ptr(), short.as_mut_ptr(), short.len() as u32) } as usize;
        if size == 0 || size >= short.len() { return Err("Trusted CLI path has no supported literal Windows invocation".into()); }
        let short = PathBuf::from(String::from_utf16(&short[..size]).map_err(|_| DENIED)?);
        if short.canonicalize().map_err(|_| DENIED)? != path.canonicalize().map_err(|_| DENIED)?
            || file_digest(&short)? != file_digest(path)? { return Err(DENIED.into()); }
        let literal = plain_path(&short)?;
        if literal_command_path(&literal) { return Ok(literal); }
    }
    Err("Trusted CLI path has no supported literal invocation".into())
}

fn reject_links(path: &Path) -> Result<(), String> {
    for ancestor in path.ancestors() {
        let metadata = fs::symlink_metadata(ancestor).map_err(|_| DENIED)?;
        if metadata.file_type().is_symlink() {
            return Err(DENIED.into());
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err(DENIED.into());
            }
        }
    }
    Ok(())
}

fn rule_bytes(executable: &str) -> Result<Vec<u8>, String> {
    if executable.len() > 4096 || executable.contains('\0') {
        return Err(DENIED.into());
    }
    let pattern = serde_json::to_string(&[executable]).map_err(|_| DENIED)?;
    Ok(format!("prefix_rule(pattern={pattern}, decision=\"allow\")\n").into_bytes())
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| DENIED)?;
    file.write_all(bytes).map_err(|_| DENIED.into())
}

fn paths(policy: &RestrictedCliPolicy) -> (PathBuf, PathBuf, PathBuf) {
    let directory = Path::new(&policy.cwd).join(&policy.metadata_directory);
    let config = directory.join("config.toml");
    let rule = directory.join("rules").join(RULE);
    (directory, config, rule)
}

fn verify_files(policy: &RestrictedCliPolicy) -> Result<(), String> {
    if !RestrictedCliCapability::valid_metadata_directory(&policy.metadata_directory) { return Err(DENIED.into()); }
    let (directory, config, rule) = paths(policy);
    reject_links(&directory)?;
    reject_links(Path::new(&policy.executable))?;
    let mut entries = fs::read_dir(&directory)
        .map_err(|_| DENIED)?
        .map(|entry| {
            entry
                .map(|entry| entry.file_name())
                .map_err(|_| DENIED.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort();
    if entries
        != [
            std::ffi::OsString::from("config.toml"),
            std::ffi::OsString::from("rules"),
        ]
    {
        return Err(DENIED.into());
    }
    let rules = fs::read_dir(directory.join("rules"))
        .map_err(|_| DENIED)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| DENIED)?;
    if rules.len() != 1 || rules[0].file_name() != RULE {
        return Err(DENIED.into());
    }
    reject_links(&config)?;
    reject_links(&rule)?;
    if !same_bytes(&config, CONFIG)?
        || !same_bytes(&rule, &rule_bytes(&policy.executable)?)?
        || file_digest(&config)? != policy.config_sha256
        || file_digest(&rule)? != policy.rule_sha256
        || file_digest(Path::new(&policy.executable))? != policy.executable_sha256
    {
        return Err(DENIED.into());
    }
    Ok(())
}

fn same_bytes(path: &Path, expected: &[u8]) -> Result<bool, String> {
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|_| DENIED)?
        .take(expected.len() as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| DENIED)?;
    Ok(bytes == expected)
}

fn provision(
    context: &RuntimeContext,
    sandbox: &ExecutionSandboxPolicy,
    executable: &Path,
    metadata_directory: &str,
) -> Result<RestrictedCliPolicy, String> {
    if !RestrictedCliCapability::valid_metadata_directory(metadata_directory) { return Err(DENIED.into()); }
    let cwd = Path::new(&context.cwd);
    reject_links(cwd)?;
    reject_links(executable)?;
    let cwd = cwd.canonicalize().map_err(|_| DENIED)?;
    let executable = executable.canonicalize().map_err(|_| DENIED)?;
    for root in &sandbox.writable_roots {
        let root = Path::new(root).canonicalize().map_err(|_| DENIED)?;
        if executable.starts_with(root) {
            return Err("Restricted CLI executable must be outside writable roots".into());
        }
    }
    if cwd != Path::new(&sandbox.cwd).canonicalize().map_err(|_| DENIED)? {
        return Err(DENIED.into());
    }
    let executable = command_path(&executable)?;
    let rule_bytes = rule_bytes(&executable)?;
    let policy = RestrictedCliPolicy {
        tab_id: context.tab_id.clone(),
        binding_nonce: context.binding_nonce.clone(),
        epoch: context.epoch,
        cwd: context.cwd.clone(),
        metadata_directory: metadata_directory.into(),
        executable_sha256: file_digest(Path::new(&executable))?,
        executable,
        config_sha256: digest(CONFIG),
        rule_sha256: digest(&rule_bytes),
    };
    let (directory, config, rule) = paths(&policy);
    fs::create_dir(&directory).map_err(|_| DENIED)?;
    let result = (|| {
        fs::create_dir(directory.join("rules")).map_err(|_| DENIED)?;
        write_new(&config, CONFIG)?;
        write_new(&rule, &rule_bytes)?;
        verify_files(&policy)
    })();
    if result.is_err() {
        remove_owned(&policy);
    }
    result.map(|_| policy)
}

fn require_fresh_project(cwd: &Path, metadata_directory: &str) -> Result<(), String> {
    if !RestrictedCliCapability::valid_metadata_directory(metadata_directory) { return Err(DENIED.into()); }
    for ancestor in cwd.ancestors() {
        if ancestor.join(metadata_directory).try_exists().map_err(|_| DENIED)?
            || ancestor.join(".git").try_exists().map_err(|_| DENIED)?
        {
            return Err("Restricted CLI exception requires a fresh project configuration".into());
        }
    }
    Ok(())
}

fn remove_owned(policy: &RestrictedCliPolicy) {
    if !RestrictedCliCapability::valid_metadata_directory(&policy.metadata_directory) { return; }
    let (directory, config, rule) = paths(policy);
    if reject_links(&directory).is_err() {
        return;
    }
    for (path, expected) in [(rule, &policy.rule_sha256), (config, &policy.config_sha256)] {
        if reject_links(&path).is_ok()
            && fs::metadata(&path).is_ok_and(|metadata| metadata.len() <= 8192)
            && file_digest(&path).is_ok_and(|digest| &digest == expected)
        {
            let _ = fs::remove_file(path);
        }
    }
    let _ = fs::remove_dir(directory.join("rules"));
    let _ = fs::remove_dir(directory);
}

fn sandbox(
    svc: &ServiceContext,
    context: &RuntimeContext,
) -> Result<Option<ExecutionSandboxPolicy>, String> {
    let mux = svc.mux.read();
    let tab = mux.get_tab(&context.tab_id).ok_or(DENIED)?;
    if tab.session_binding_nonce.as_deref() != Some(context.binding_nonce.as_str()) {
        return Err(DENIED.into());
    }
    if tab.neighbor_authority.is_none() {
        return Ok(None);
    }
    if tab.restorable_launch.permission_bypass_requested {
        if tab.restorable_launch.execution_sandbox.is_some() {
            return Err(DENIED.into());
        }
        drop(mux);
        svc.require_neighbor_restore_grant(&context.tab_id, true)?;
        return Ok(None);
    }
    let policy = tab
        .restorable_launch
        .execution_sandbox
        .as_ref()
        .ok_or(DENIED)?;
    if policy.intent != ExecutionIntent::Mutating
        || policy.runtime_mode.as_deref() != Some("workspace-write")
    {
        return Ok(None);
    }
    crate::service::execution_sandbox::validate_runtime_settings(
        policy,
        &context.settings,
        &context.cwd,
    )?;
    Ok(Some(policy.clone()))
}

fn metadata_directory(svc: &ServiceContext, provider: &str) -> Result<Option<String>, String> {
    let capability = svc.agent_registry.with_provider(provider, |provider| {
        provider.agent_runtime().and_then(|capability| capability.restricted_cli)
    }).flatten();
    capability.map(|capability| {
        let directory = capability.metadata_directory();
        if !RestrictedCliCapability::valid_metadata_directory(directory) { return Err(DENIED.into()); }
        Ok(directory.to_owned())
    }).transpose()
}

pub(super) fn prepare(
    svc: &ServiceContext,
    provider: &str,
    context: &RuntimeContext,
) -> Result<Option<RestrictedCliPolicy>, String> {
    if !cfg!(windows) {
        return Ok(None);
    }
    let Some(metadata_directory) = metadata_directory(svc, provider)? else {
        return Ok(None);
    };
    let Some(sandbox) = sandbox(svc, context)? else {
        return Ok(None);
    };
    let executable = crate::service::instance_env::instance_cli_binary_path().ok_or(DENIED)?;
    let existing = LEASES.lock().get(&context.tab_id).cloned();
    if let Some(mut lease) = existing {
        if lease.provider != provider
            || lease.sandbox != sandbox
            || lease.policy.metadata_directory != metadata_directory
            || lease.policy.binding_nonce != context.binding_nonce
            || command_path(&executable.canonicalize().map_err(|_| DENIED)?)?
                != lease.policy.executable
        {
            return Err(DENIED.into());
        }
        verify_files(&lease.policy)?;
        let mut leases = LEASES.lock();
        if leases
            .get(&context.tab_id)
            .is_none_or(|current| current.policy != lease.policy)
        {
            return Err(DENIED.into());
        }
        lease.policy.epoch = context.epoch;
        let policy = lease.policy.clone();
        leases.insert(context.tab_id.clone(), lease);
        record_policy(&policy);
        return Ok(Some(policy));
    }
    require_fresh_project(Path::new(&context.cwd), &metadata_directory)?;
    let executable_pin = pin_executable(&executable)?;
    let policy = provision(context, &sandbox, &executable, &metadata_directory)?;
    if !self::sandbox(svc, context).is_ok_and(|current| current.as_ref() == Some(&sandbox))
        || !self::metadata_directory(svc, provider).is_ok_and(|current| current.as_deref() == Some(metadata_directory.as_str())) {
        remove_owned(&policy);
        return Err(DENIED.into());
    }
    LEASES.lock().insert(
        context.tab_id.clone(),
        Lease {
            policy: policy.clone(),
            sandbox,
            provider: provider.into(),
            _executable_pin: executable_pin,
        },
    );
    record_policy(&policy);
    Ok(Some(policy))
}

pub(super) fn validate(svc: &ServiceContext, context: &RuntimeContext) -> Result<(), String> {
    let Some(policy) = &context.restricted_cli else {
        return Ok(());
    };
    let current = sandbox(svc, context)?.ok_or(DENIED)?;
    let lease = LEASES.lock().get(&context.tab_id).cloned().ok_or(DENIED)?;
    if lease.policy != *policy || lease.sandbox != current || !binding_matches(policy, context)
        || metadata_directory(svc, &lease.provider)?.as_deref() != Some(policy.metadata_directory.as_str()) {
        return Err(DENIED.into());
    }
    let executable = crate::service::instance_env::instance_cli_binary_path().ok_or(DENIED)?;
    if command_path(&executable.canonicalize().map_err(|_| DENIED)?)? != policy.executable {
        return Err(DENIED.into());
    }
    verify_files(policy)
}

fn binding_matches(policy: &RestrictedCliPolicy, context: &RuntimeContext) -> bool {
    policy.epoch == context.epoch
        && policy.tab_id == context.tab_id
        && policy.binding_nonce == context.binding_nonce
        && policy.cwd == context.cwd
}

pub(super) fn validate_binding(
    svc: &ServiceContext,
    context: &RuntimeContext,
) -> Result<(), String> {
    let Some(policy) = &context.restricted_cli else {
        return Ok(());
    };
    let current = sandbox(svc, context)?.ok_or(DENIED)?;
    let leases = LEASES.lock();
    let lease = leases.get(&context.tab_id).ok_or(DENIED)?;
    if lease.policy != *policy || lease.sandbox != current || !binding_matches(policy, context)
        || metadata_directory(svc, &lease.provider)?.as_deref() != Some(policy.metadata_directory.as_str()) {
        return Err(DENIED.into());
    }
    Ok(())
}

pub(super) async fn validate_before_operation(
    svc: &ServiceContext,
    context: &RuntimeContext,
) -> Result<(), RuntimeFailure> {
    if context.restricted_cli.is_none() {
        return Ok(());
    }
    let svc = svc.clone();
    let context = context.clone();
    tokio::task::spawn_blocking(move || validate(&svc, &context))
        .await
        .map_err(|_| RuntimeFailure::Superseded)?
        .map_err(RuntimeFailure::Refused)
}

pub(super) fn revoke(tab: &str) -> Result<(), String> {
    let lease = LEASES.lock().remove(tab);
    let Some(lease) = lease else {
        return Ok(());
    };
    remove_owned(&lease.policy);
    let (directory, _, _) = paths(&lease.policy);
    let removed = fs::symlink_metadata(directory)
        .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound);
    #[cfg(debug_assertions)]
    log::debug!(
        "[runtime-policy] phase=cli_exception_revoke tab={} epoch={} removed={}",
        tab,
        lease.policy.epoch,
        removed
    );
    if removed {
        Ok(())
    } else {
        Err(
            "Restricted CLI policy cleanup incomplete; preserved unmatched or inaccessible files"
                .into(),
        )
    }
}

#[cfg(test)]
mod tests;

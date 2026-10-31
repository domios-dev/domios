use crate::args::mem::MemBackendAction;
use codeterm_types::server_bootstrap::{
    CliResourceHint, ServerComponentManifest, API_VERSION, BOOTSTRAP_PROTOCOL,
    CLI_RESOURCE_HINT_FILE, SERVER_COMPONENT_SCHEMA,
};
use std::{io::Read, path::Path, process::Command};

pub(crate) fn argv(action: &MemBackendAction, json: bool) -> Vec<String> {
    let MemBackendAction::Migrate { to, into_nonempty } = action;
    let mut args = vec!["memory-migrate".into(), "--to".into(), to.clone()];
    if *into_nonempty {
        args.push("--into-nonempty".into());
    }
    if json {
        args.push("--json".into());
    }
    args
}

#[derive(Debug)]
struct ServerHintError;
impl std::fmt::Display for ServerHintError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("memory_migration_server_hint: missing, stale or unverifiable server resource hint; reinstall the CLI from the installed Domios application")
    }
}

fn server(executable: &Path) -> Result<std::path::PathBuf, ServerHintError> {
    verified_server(executable).map_err(|_| ServerHintError)
}

fn digest(path: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    std::io::copy(
        &mut std::fs::File::open(path).map_err(|error| error.to_string())?,
        &mut hash,
    )
    .map_err(|error| error.to_string())?;
    Ok(format!("{:x}", hash.finalize()))
}

fn read_bounded(path: &Path) -> Result<Vec<u8>, String> {
    if !path
        .symlink_metadata()
        .map_err(|error| error.to_string())?
        .file_type()
        .is_file()
    {
        return Err("Expected a regular resource file".into());
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|error| error.to_string())?
        .take(8193)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > 8192 {
        return Err("Resource file exceeds bound".into());
    }
    Ok(bytes)
}

fn verified_server(executable: &Path) -> Result<std::path::PathBuf, String> {
    let executable = executable
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let directory = executable.parent().ok_or("CLI directory unavailable")?;
    let hint: CliResourceHint =
        serde_json::from_slice(&read_bounded(&directory.join(CLI_RESOURCE_HINT_FILE))?)
            .map_err(|error| error.to_string())?;
    if !hint.resources.is_absolute() || digest(&executable)? != hint.cli_sha256 {
        return Err("Stale CLI resource hint".into());
    }
    let resources = hint
        .resources
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let packaged_cli = resources.join(format!("cli/codeterm{}", std::env::consts::EXE_SUFFIX));
    if digest(&packaged_cli)? != hint.cli_sha256 {
        return Err("Packaged CLI differs from installed alias".into());
    }
    let manifest: ServerComponentManifest =
        serde_json::from_slice(&read_bounded(&resources.join("server-component.json"))?)
            .map_err(|error| error.to_string())?;
    if manifest.schema != SERVER_COMPONENT_SCHEMA
        || manifest.api_version != API_VERSION
        || manifest.bootstrap_protocol != BOOTSTRAP_PROTOCOL
        || manifest.server_build != env!("CODETERM_CLI_BUILD_SHA")
    {
        return Err("Incompatible server manifest".into());
    }
    let server = codeterm_process_identity::client_identity::resolve_server_executable(
        &resources,
        &manifest.server_executable,
        &manifest.client_identity,
    )
    .map_err(|error| error.to_string())?;
    if digest(&server)? != manifest.server_sha256 {
        return Err("Server digest mismatch".into());
    }
    Ok(server)
}

fn delegate(server: &Path, action: &MemBackendAction, json: bool, data_root: &Path) -> Command {
    let mut command = Command::new(server);
    command
        .args(argv(action, json))
        .env("CODETERM_DATA_DIR", data_root);
    command
}

pub(crate) fn handle(json: bool, action: MemBackendAction) -> ! {
    let data_root = crate::data_root();
    if !data_root.is_absolute() {
        crate::cmd::print::fail(
            1,
            "memory_migration_instance_paths: data directory must be absolute",
        );
    }
    let result = std::env::current_exe()
        .map_err(|error| error.to_string())
        .and_then(|executable| server(&executable).map_err(|error| error.to_string()))
        .and_then(|server| {
            delegate(&server, &action, json, &data_root)
                .status()
                .map_err(|error| error.to_string())
        });
    match result {
        Ok(status) => std::process::exit(status.code().unwrap_or(1)),
        Err(error) => crate::cmd::print::fail(1, error),
    }
}

#[cfg(test)]
mod tests;

use codeterm_types::server_bootstrap::{InstanceSelector, LaunchOptions};
use serde::Deserialize;
use std::{ffi::OsString, io, io::Read, path::PathBuf};

const ARGUMENT: &str = "--isolated-update-launch=";
const FILE_NAME: &str = ".domios-isolated-update.json";
const IDENTIFIER: &str = "com.domios.dev.updater";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct IsolatedUpdateLaunch {
    install_root: PathBuf,
    instance: String,
    data_dir: PathBuf,
    config_dir: PathBuf,
    port: u16,
    fixture_root: PathBuf,
}

pub(super) fn from_arguments(
    arguments: &[OsString],
    development: bool,
    config: &tauri::utils::config::Config,
) -> io::Result<Option<IsolatedUpdateLaunch>> {
    if !arguments.iter().any(|arg| {
        arg.to_str()
            .is_some_and(|arg| arg.starts_with("--isolated-update-launch"))
    }) {
        return Ok(None);
    }
    if !development {
        return Err(io::Error::other(
            "Isolated update launch requires a debug artifact",
        ));
    }
    if arguments.len() != 1
        || config.identifier != IDENTIFIER
        || config.product_name.as_deref() != Some("Domios Update Proof")
        || config.main_binary_name.as_deref() != Some("domios-dev-app")
    {
        return Err(io::Error::other(
            "Isolated update launch requires the dedicated DEV bundle",
        ));
    }
    let file = arguments[0]
        .to_str()
        .and_then(|arg| arg.strip_prefix(ARGUMENT))
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other("Invalid isolated update launch argument"))?;
    if !file.is_absolute()
        || file.file_name().is_none_or(|name| name != FILE_NAME)
        || !file.symlink_metadata()?.file_type().is_file()
    {
        return Err(io::Error::other(
            "Isolated update context must be a regular install-root file",
        ));
    }
    let executable = std::env::current_exe()?.canonicalize()?;
    let root = executable
        .parent()
        .ok_or_else(|| io::Error::other("Missing install root"))?;
    if file.canonicalize()? != root.join(FILE_NAME) {
        return Err(io::Error::other(
            "Isolated update context belongs to another install root",
        ));
    }
    let mut bytes = Vec::new();
    std::fs::File::open(file)?
        .take(8193)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 8192 {
        return Err(io::Error::other("Isolated update context exceeds limit"));
    }
    let launch: IsolatedUpdateLaunch = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
    let data = launch.data_dir.canonicalize()?;
    let config = launch.config_dir.canonicalize()?;
    if launch.port < 1024
        || launch.install_root.canonicalize()? != root
        || data.starts_with(root)
        || config.starts_with(root)
        || launch.fixture_root.canonicalize()? != data.join("update-fixtures")
    {
        return Err(io::Error::other(
            "Isolated update paths do not match the install context",
        ));
    }
    LaunchOptions {
        instance: launch.selector(),
        resources: root.into(),
        probe_sink: None,
        probe_identity_alias: None,
        development_owner: None,
    }
    .validate(true)?;
    Ok(Some(launch))
}

impl IsolatedUpdateLaunch {
    pub(super) fn selector(&self) -> InstanceSelector {
        InstanceSelector::IsolatedVerification {
            instance: Some(self.instance.clone()),
            data_dir: self.data_dir.clone(),
            config_dir: self.config_dir.clone(),
            bind: ([127, 0, 0, 1], self.port).into(),
        }
    }

    pub(super) fn install_environment(&self) {
        std::env::set_var("CODETERM_INSTANCE", &self.instance);
        std::env::set_var("CODETERM_DATA_DIR", &self.data_dir);
        std::env::set_var("CODETERM_CONFIG_DIR", &self.config_dir);
        std::env::set_var("CODETERM_DEV_UPDATE_FIXTURES", &self.fixture_root);
    }
}

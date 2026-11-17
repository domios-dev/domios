use codeterm_plugin::provider_account::{
    storage::{
        AccountCapability, SharedStoragePath, StorageConfiguration, StorageConfigurationFormat,
        StoragePathKind,
    },
    InvocationContext, SharedReadGrant,
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};

const MAX_CONFIG_BYTES: u64 = 256 * 1024;

pub(super) fn usual_home(capability: &AccountCapability) -> Result<PathBuf, String> {
    if let Some(value) =
        std::env::var_os(&capability.home_environment).filter(|value| !value.is_empty())
    {
        return absolute(PathBuf::from(value));
    }
    let path = if let Some(relative) = capability.default_home.strip_prefix("~/") {
        dirs::home_dir()
            .ok_or("User home is unavailable")?
            .join(relative)
    } else {
        PathBuf::from(&capability.default_home)
    };
    absolute(path)
}

fn absolute(path: PathBuf) -> Result<PathBuf, String> {
    if !path.is_absolute()
        || path.to_str().is_none()
        || path
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err("Account storage requires an absolute path without parent traversal".into());
    }
    Ok(path)
}

fn relative(base: &Path, path: &str) -> PathBuf {
    path.split(['/', '\\'])
        .filter(|part| !part.is_empty())
        .fold(base.to_owned(), |base, part| base.join(part))
}

fn private_directory(path: &Path) -> Result<(), String> {
    match codeterm_util::private_file::create_dir_owner_only(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            codeterm_util::private_file::open_directory_owner_only(path)
                .map(|_| ())
                .map_err(|error| error.to_string())
        }
        Err(error) => Err(error.to_string()),
    }
}

fn configuration_value(
    home: &Path,
    config: &StorageConfiguration,
) -> Result<Option<String>, String> {
    let path = relative(home, &config.file);
    let text = match bounded_read(&path)? {
        Some(text) => text,
        None => return Ok(None),
    };
    match config.format {
        StorageConfigurationFormat::Toml => {
            let value: toml::Value =
                toml::from_str(&text).map_err(|_| "Invalid provider storage configuration")?;
            value
                .get(&config.key)
                .map(|value| {
                    value
                        .as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| "Provider storage root must be a path".into())
                })
                .transpose()
        }
        StorageConfigurationFormat::Json => {
            let value: serde_json::Value = serde_json::from_str(&text)
                .map_err(|_| "Invalid provider storage configuration")?;
            value
                .get(&config.key)
                .map(|value| {
                    value
                        .as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| "Provider storage root must be a path".into())
                })
                .transpose()
        }
    }
}

fn bounded_read(path: &Path) -> Result<Option<String>, String> {
    use std::io::Read;
    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    if file.metadata().map_err(|error| error.to_string())?.len() > MAX_CONFIG_BYTES {
        return Err("Provider storage configuration exceeds its size bound".into());
    }
    let mut text = String::new();
    file.by_ref()
        .take(MAX_CONFIG_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|_| "Provider storage configuration could not be read")?;
    if text.len() as u64 > MAX_CONFIG_BYTES {
        return Err("Provider storage configuration exceeds its size bound".into());
    }
    Ok(Some(text))
}

fn shared_root(layout: &SharedStoragePath, usual: &Path) -> Result<PathBuf, String> {
    if let Some(variable) = &layout.root_environment {
        if let Some(root) = std::env::var_os(variable).filter(|value| !value.is_empty()) {
            return absolute(root.into());
        }
    }
    if let Some(config) = &layout.configuration {
        if let Some(root) = configuration_value(usual, config)? {
            return absolute(root.into());
        }
    }
    Ok(usual.into())
}

fn pin_configuration(
    home: &Path,
    config: &StorageConfiguration,
    root: &Path,
) -> Result<(), String> {
    if configuration_value(home, config)?.as_deref() == root.to_str() {
        return Ok(());
    }
    let path = relative(home, &config.file);
    if std::fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err("Account configuration cannot be a symbolic link".into());
    }
    let text = bounded_read(&path)?.unwrap_or_default();
    let root = root
        .to_str()
        .ok_or("Provider storage path is not Unicode")?;
    let bytes = match config.format {
        StorageConfigurationFormat::Toml => {
            let mut value: toml::Value = if text.is_empty() {
                toml::Value::Table(Default::default())
            } else {
                toml::from_str(&text).map_err(|_| "Invalid account configuration")?
            };
            value
                .as_table_mut()
                .ok_or("Account configuration must be a table")?
                .insert(config.key.clone(), toml::Value::String(root.into()));
            toml::to_string(&value)
                .map_err(|error| error.to_string())?
                .into_bytes()
        }
        StorageConfigurationFormat::Json => {
            let mut value = if text.is_empty() {
                serde_json::json!({})
            } else {
                serde_json::from_str(&text).map_err(|_| "Invalid account configuration")?
            };
            value
                .as_object_mut()
                .ok_or("Account configuration must be an object")?
                .insert(config.key.clone(), serde_json::Value::String(root.into()));
            serde_json::to_vec_pretty(&value).map_err(|error| error.to_string())?
        }
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    codeterm_util::private_file::write_atomic(&path, &bytes).map_err(|error| error.to_string())
}

fn link_shared(target: &Path, link: &Path, directory: bool) -> Result<(), String> {
    if std::fs::symlink_metadata(link).is_ok() {
        if std::fs::canonicalize(target)
            .ok()
            .is_some_and(|target| std::fs::canonicalize(link).ok().as_ref() == Some(&target))
        {
            return Ok(());
        }
        return Err("Account history path does not point to its declared shared storage".into());
    }
    if let Some(parent) = link.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    #[cfg(unix)]
    let result = std::os::unix::fs::symlink(target, link);
    #[cfg(windows)]
    let result = if directory {
        std::os::windows::fs::symlink_dir(target, link)
    } else {
        std::os::windows::fs::symlink_file(target, link)
    };
    #[cfg(unix)]
    let _ = directory;
    result.map_err(|error| format!("Shared account history could not be linked: {error}"))
}

fn home_path(data_root: &Path, provider: &str, account: &str) -> Result<PathBuf, String> {
    codeterm_plugin::plugin_id::validate(provider)?;
    if !codeterm_types::config::provider_accounts::valid_account_id(account) || account == "default"
    {
        return Err("A named account requires a non-default portable identity".into());
    }
    absolute(data_root.into())?;
    Ok(data_root
        .join("provider-accounts")
        .join(provider)
        .join(account))
}

pub(super) fn discard(data_root: &Path, provider: &str, account: &str) -> Result<(), String> {
    let home = home_path(data_root, provider, account)?;
    match std::fs::symlink_metadata(&home) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.to_string()),
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err("The account home was replaced; credentials were retained".into())
        }
        Ok(_) => {}
    }
    let host_root = std::fs::canonicalize(data_root).map_err(|error| error.to_string())?;
    let parent = home
        .parent()
        .ok_or("The account home has no owner directory")?;
    let parent_path = std::fs::canonicalize(parent).map_err(|error| error.to_string())?;
    if parent_path != host_root.join("provider-accounts").join(provider) {
        return Err("The account home is outside its owner directory".into());
    }
    for path in [parent, home.as_path()] {
        codeterm_util::private_file::open_directory_owner_only(path)
            .map_err(|error| error.to_string())?;
    }
    let resolved = std::fs::canonicalize(&home).map_err(|error| error.to_string())?;
    if resolved.parent() != Some(parent_path.as_path())
        || resolved.file_name() != Some(std::ffi::OsStr::new(account))
    {
        return Err("The account home changed; credentials were retained".into());
    }
    std::fs::remove_dir_all(home)
        .map_err(|error| format!("Account credentials could not be removed: {error}"))
}

#[cfg(test)]
pub(super) fn provision(
    data_root: &Path,
    provider: &str,
    account: &str,
    capability: &AccountCapability,
    usual: &Path,
) -> Result<Arc<InvocationContext>, String> {
    provision_namespace(data_root, provider, account, account, capability, usual)
}

pub(super) fn provision_namespace(
    data_root: &Path,
    provider: &str,
    account: &str,
    namespace: &str,
    capability: &AccountCapability,
    usual: &Path,
) -> Result<Arc<InvocationContext>, String> {
    capability.validate()?;
    validate_environment_authority(capability)?;
    validate_isolation(capability)?;
    let home = home_path(data_root, provider, namespace)?;
    absolute(usual.into())?;
    let accounts_root = data_root.join("provider-accounts");
    private_directory(&accounts_root)?;
    let provider_root = accounts_root.join(provider);
    private_directory(&provider_root)?;
    private_directory(&home)?;
    let canonical_home = std::fs::canonicalize(&home).map_err(|error| error.to_string())?;
    for isolated in &capability.isolated_paths {
        let path = relative(&home, isolated);
        if !codeterm_util::project_roots::canonical_target(&path)
            .is_some_and(|path| path.starts_with(&canonical_home))
        {
            return Err("An isolated account path escapes its account home".into());
        }
    }
    let mut environment = BTreeMap::from([(
        capability.home_environment.clone(),
        Some(home.to_string_lossy().into_owned()),
    )]);
    let mut grants = Vec::new();
    for layout in &capability.shared_paths {
        if layout
            .configuration
            .as_ref()
            .is_some_and(|config| !capability.isolated_paths.contains(&config.file))
        {
            return Err("Shared storage configuration must belong to the isolated account".into());
        }
        let root = shared_root(layout, usual)?;
        std::fs::create_dir_all(&root).map_err(|error| error.to_string())?;
        grants.push(SharedReadGrant::from_layout(layout, &root)?);
        match layout.kind {
            StoragePathKind::VersionedSqlite => {
                let variable = layout
                    .root_environment
                    .as_ref()
                    .ok_or("Shared SQLite requires a declared root selector")?;
                environment.insert(variable.clone(), Some(root.to_string_lossy().into_owned()));
                let config = layout
                    .configuration
                    .as_ref()
                    .ok_or("Shared SQLite requires a declared configuration selector")?;
                pin_configuration(&home, config, &root)?;
            }
            StoragePathKind::Directory | StoragePathKind::File => {
                let target = relative(&root, &layout.path);
                let directory = layout.kind == StoragePathKind::Directory;
                if directory {
                    std::fs::create_dir_all(&target).map_err(|error| error.to_string())?;
                } else {
                    if let Some(parent) = target.parent() {
                        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
                    }
                    match codeterm_util::private_file::create_new_owner_only(&target) {
                        Ok(_) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                        Err(error) => return Err(error.to_string()),
                    }
                    if !std::fs::metadata(&target).is_ok_and(|metadata| metadata.is_file()) {
                        return Err("Shared history must be a regular file".into());
                    }
                }
                link_shared(&target, &relative(&home, &layout.path), directory)?;
            }
        }
    }
    InvocationContext::from_capability(
        provider.into(),
        account.into(),
        capability,
        environment,
        vec![home],
        vec![usual.into(), accounts_root],
    )?
    .with_shared_reads(grants)
}

fn validate_environment_authority(capability: &AccountCapability) -> Result<(), String> {
    let names = std::iter::once(capability.home_environment.as_str())
        .chain(
            capability
                .shared_paths
                .iter()
                .filter_map(|path| path.root_environment.as_deref()),
        )
        .chain(capability.secret_environment.iter().map(String::as_str))
        .chain(capability.excluded_environment.iter().map(String::as_str));
    for name in names {
        let reserved = codeterm_util::tab_launch::MARKERS
            .iter()
            .any(|host| host.eq_ignore_ascii_case(name))
            || [
                "HOME",
                "USERPROFILE",
                "PATH",
                "CODETERM_API_TOKEN",
                "CODETERM_API_URL",
                "CODETERM_API_PORT",
                "CODETERM_SOCKET_PATH",
            ]
            .iter()
            .any(|host| host.eq_ignore_ascii_case(name));
        if reserved
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            || !name
                .as_bytes()
                .first()
                .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_')
        {
            return Err("Account storage cannot replace host identity or the process home".into());
        }
    }
    if capability
        .secret_environment
        .iter()
        .chain(&capability.excluded_environment)
        .any(|name| name.eq_ignore_ascii_case(&capability.home_environment))
    {
        return Err("Account home cannot be removed by its credential policy".into());
    }
    Ok(())
}

fn validate_isolation(capability: &AccountCapability) -> Result<(), String> {
    let normalized = |path: &str| {
        let path = path
            .split(['/', '\\'])
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("/");
        if cfg!(windows) {
            path.to_ascii_lowercase()
        } else {
            path
        }
    };
    for layout in &capability.shared_paths {
        let shared = normalized(&layout.path);
        for isolated in &capability.isolated_paths {
            let isolated = normalized(isolated);
            let overlap = match layout.kind {
                StoragePathKind::Directory => {
                    isolated == shared
                        || isolated
                            .strip_prefix(&shared)
                            .is_some_and(|tail| tail.starts_with('/'))
                }
                StoragePathKind::File => isolated == shared,
                StoragePathKind::VersionedSqlite => isolated
                    .strip_prefix(&shared)
                    .and_then(|tail| {
                        tail.strip_suffix("-wal")
                            .or_else(|| tail.strip_suffix("-shm"))
                            .or(Some(tail))
                    })
                    .and_then(|tail| tail.strip_suffix(".sqlite"))
                    .is_some_and(|version| {
                        !version.is_empty() && version.bytes().all(|byte| byte.is_ascii_digit())
                    }),
            };
            if overlap {
                return Err("Credential and configuration paths cannot be shared history".into());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;

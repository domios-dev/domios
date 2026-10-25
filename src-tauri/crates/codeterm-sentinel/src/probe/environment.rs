use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct Environment {
    _deny_directory: Option<std::sync::Arc<tempfile::TempDir>>,
    pub(super) variables: Vec<(OsString, OsString)>,
}

impl Default for Environment {
    fn default() -> Self {
        Self::for_command(std::env::vars_os(), None, &[])
    }
}

impl Environment {
    pub fn resolve_program(&self, names: &[&str]) -> Option<PathBuf> {
        let path = self
            .variables
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("PATH"))?;
        for directory in std::env::split_paths(&path.1) {
            for name in names {
                let candidate = directory.join(name);
                if candidate.is_file() {
                    if let Ok(path) = candidate.canonicalize() {
                        return Some(path);
                    }
                }
            }
        }
        None
    }

    pub fn for_command(
        inherited: impl IntoIterator<Item = (OsString, OsString)>,
        instance_cli: Option<&Path>,
        path_defaults: &[&str],
    ) -> Self {
        Self::for_command_on_platform(inherited, instance_cli, path_defaults, cfg!(windows))
    }

    fn for_command_on_platform(
        inherited: impl IntoIterator<Item = (OsString, OsString)>,
        instance_cli: Option<&Path>,
        path_defaults: &[&str],
        windows: bool,
    ) -> Self {
        let inherited: Vec<_> = inherited.into_iter().collect();
        let mut cli_dirs = Vec::new();
        if let Some(directory) = instance_cli.and_then(Path::parent) {
            cli_dirs.push(directory.to_path_buf());
        }
        for (key, value) in &inherited {
            if matches!(
                key.to_string_lossy().to_ascii_uppercase().as_str(),
                "CODETERM_CLI_PATH" | "DOMIOS_CLI_PATH"
            ) {
                if let Some(directory) = Path::new(value).parent() {
                    cli_dirs.push(directory.to_path_buf());
                }
            }
        }
        let inherited_path = inherited
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case("PATH"))
            .map(|(_, value)| value.as_os_str());
        let path = probe_path(inherited_path, path_defaults).unwrap_or_default();
        let mut paths: Vec<PathBuf> = std::env::split_paths(&path).collect();
        let mut variables = vec![("LANG".into(), "C.UTF-8".into())];
        let identity: &[&str] = if windows {
            &[
                "USERNAME",
                "USERPROFILE",
                "HOMEDRIVE",
                "HOMEPATH",
                "TEMP",
                "TMP",
                "APPDATA",
                "LOCALAPPDATA",
            ]
        } else {
            &["HOME", "USER", "LOGNAME", "TMPDIR"]
        };
        variables.extend(
            inherited
                .iter()
                .filter(|(key, value)| {
                    !value.is_empty() && identity.iter().any(|name| key.eq_ignore_ascii_case(name))
                })
                .cloned(),
        );
        if let Some((_, root)) = inherited
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case("SystemRoot"))
            .filter(|_| windows)
        {
            let root_path = PathBuf::from(root);
            if root_path.is_absolute() {
                let system = root_path.join("System32");
                if !paths.iter().any(|path| same_directory(path, &system)) {
                    paths.push(system.clone());
                }
                variables.push(("SystemRoot".into(), root.clone()));
                variables.push(("COMSPEC".into(), system.join("cmd.exe").into_os_string()));
                variables.push((
                    "PATHEXT".into(),
                    inherited
                        .iter()
                        .find(|(key, _)| key.eq_ignore_ascii_case("PATHEXT"))
                        .filter(|(_, value)| !value.is_empty())
                        .map(|(_, value)| value.clone())
                        .unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".into()),
                ));
            }
        }
        paths.retain(|path| {
            is_shared_directory(path) || !cli_dirs.iter().any(|cli| same_directory(path, cli))
        });
        let deny_directory = if paths.iter().any(|path| holds_domios_cli(path)) {
            match deny_directory(windows) {
                Ok(directory) => Some(std::sync::Arc::new(directory)),
                Err(error) => {
                    log::warn!("[blueprint] CLI accident prevention unavailable: {error}");
                    None
                }
            }
        } else {
            None
        };
        if let Some(directory) = &deny_directory {
            paths.insert(0, directory.path().to_path_buf());
        }
        variables.push((
            "PATH".into(),
            std::env::join_paths(paths).unwrap_or_default(),
        ));
        Self {
            variables,
            _deny_directory: deny_directory,
        }
    }

    #[cfg(windows)]
    pub fn windows_block(&self) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        let mut values: Vec<_> = self.variables.iter().collect();
        values.sort_by_key(|(key, _)| key.to_string_lossy().to_lowercase());
        let mut block = Vec::new();
        for (key, value) in values {
            block.extend(key.encode_wide());
            block.push('=' as u16);
            block.extend(value.encode_wide());
            block.push(0);
        }
        block.push(0);
        if block.len() == 1 {
            block.push(0);
        }
        block
    }
}

fn is_shared_directory(path: &Path) -> bool {
    ["/usr/bin", "/bin", "/usr/local/bin", "/opt/homebrew/bin"]
        .iter()
        .any(|shared| same_directory(path, Path::new(shared)))
        || std::env::var_os("SystemRoot")
            .is_some_and(|root| same_directory(path, &PathBuf::from(root).join("System32")))
}

fn deny_directory(windows: bool) -> std::io::Result<tempfile::TempDir> {
    let directory = tempfile::Builder::new()
        .prefix("blueprint-cli-deny-")
        .tempdir()?;
    for name in ["domios", "codeterm"] {
        let path = directory.path().join(if windows {
            format!("{name}.cmd")
        } else {
            name.into()
        });
        std::fs::write(
            &path,
            if windows {
                "@echo off\r\necho Domios CLI is unavailable in Blueprint commands 1>&2\r\nexit /b 126\r\n"
            } else {
                "#!/bin/sh\necho 'Domios CLI is unavailable in Blueprint commands' >&2\nexit 126\n"
            },
        )?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
        }
    }
    Ok(directory)
}

fn holds_domios_cli(directory: &Path) -> bool {
    ["domios", "codeterm"].iter().any(|name| {
        ["", ".exe", ".cmd", ".bat", ".ps1"]
            .iter()
            .any(|extension| directory.join(format!("{name}{extension}")).is_file())
    })
}

fn same_directory(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ if cfg!(windows) => left
            .to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy()),
        _ => left == right,
    }
}

pub fn probe_path(inherited: Option<&OsStr>, defaults: &[&str]) -> Option<OsString> {
    let mut paths: Vec<_> = std::env::split_paths(inherited.unwrap_or_default()).collect();
    for directory in defaults {
        let directory = PathBuf::from(directory);
        if !paths.contains(&directory) {
            paths.push(directory);
        }
    }
    std::env::join_paths(paths).ok()
}

#[cfg(test)]
mod command_tests;
#[cfg(test)]
mod tests;

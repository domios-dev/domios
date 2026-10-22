use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct Environment {
    pub(super) variables: Vec<(OsString, OsString)>,
}

impl Default for Environment {
    fn default() -> Self {
        Self::for_command(std::env::vars_os(), None, &[])
    }
}

impl Environment {
    pub fn isolated(directory: &Path) -> Self {
        Self {
            variables: vec![
                ("PATH".into(), directory.as_os_str().to_owned()),
                ("HOME".into(), directory.as_os_str().to_owned()),
                ("TMPDIR".into(), directory.as_os_str().to_owned()),
                ("LANG".into(), "C.UTF-8".into()),
            ],
        }
    }

    pub fn for_command(
        inherited: impl IntoIterator<Item = (OsString, OsString)>,
        instance_cli: Option<&Path>,
        path_defaults: &[&str],
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
        let mut variables = Vec::new();
        for (key, value) in inherited {
            if is_instance_variable(&key) {
                continue;
            }
            if key.to_string_lossy().eq_ignore_ascii_case("PATH") {
                let path = probe_path(Some(&value), path_defaults).unwrap_or_default();
                let paths = std::env::split_paths(&path)
                    .filter(|path| !cli_dirs.iter().any(|cli| same_directory(path, cli)));
                variables.push((key, std::env::join_paths(paths).unwrap_or_default()));
            } else {
                variables.push((key, value));
            }
        }
        Self { variables }
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

fn is_instance_variable(key: &OsStr) -> bool {
    let key = key.to_string_lossy().to_ascii_uppercase();
    key == "CODETERM"
        || key == "DOMIOS"
        || key.starts_with("CODETERM_")
        || key.starts_with("DOMIOS_")
}

fn same_directory(left: &Path, right: &Path) -> bool {
    let left: PathBuf = left.components().collect();
    let right: PathBuf = right.components().collect();
    if cfg!(windows) {
        left.to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy())
    } else {
        left == right
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
mod tests;

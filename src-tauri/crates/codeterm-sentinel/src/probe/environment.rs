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
                (
                    "OPENSSL_CONF".into(),
                    directory.join("openssl.cnf").into_os_string(),
                ),
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
        let mut paths: Vec<PathBuf> = if cfg!(windows) {
            Vec::new()
        } else {
            vec!["/usr/bin".into(), "/bin".into()]
        };
        paths.extend(path_defaults.iter().map(PathBuf::from));
        let mut variables = vec![("LANG".into(), "C.UTF-8".into())];
        #[cfg(windows)]
        if let Some((_, root)) = inherited
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case("SystemRoot"))
        {
            let root_path = PathBuf::from(root);
            if root_path.is_absolute() {
                paths.push(root_path.join("System32"));
                variables.push(("SystemRoot".into(), root.clone()));
            }
        }
        paths.retain(|path| {
            path.is_absolute()
                && !cli_dirs.iter().any(|cli| same_directory(path, cli))
                && !["codeterm", "domios", "codeterm.exe", "domios.exe"]
                    .iter()
                    .any(|name| path.join(name).is_file())
        });
        paths.dedup();
        variables.push((
            "PATH".into(),
            std::env::join_paths(paths).unwrap_or_default(),
        ));
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

use std::ffi::{OsStr, OsString};
use std::path::Path;

#[derive(Clone, Default)]
pub(crate) struct Environment {
    pub(super) variables: Vec<(OsString, OsString)>,
    pub(super) removed: Vec<OsString>,
}

impl Environment {
    pub(crate) fn current_instance() -> Self {
        let instance = crate::service::instance_env::InstanceEnv::resolve();
        let cli = crate::service::instance_env::instance_cli_binary_path();
        let inherited = std::env::var_os("PATH");
        let path = probe_path(
            inherited.as_deref(),
            if cfg!(target_os = "macos") {
                &["/opt/homebrew/bin", "/usr/local/bin"]
            } else {
                &[]
            },
        );
        Self::for_instance(
            &instance.socket_path,
            &instance.api_url,
            &instance.api_port,
            instance.api_token.as_deref(),
            cli.as_deref(),
            path.as_deref(),
        )
    }

    pub(super) fn for_instance(
        socket_path: &str,
        api_url: &str,
        api_port: &str,
        api_token: Option<&str>,
        cli_binary: Option<&Path>,
        inherited_path: Option<&OsStr>,
    ) -> Self {
        let mut environment = Self::default();
        environment.set("CODETERM_SOCKET_PATH", socket_path);
        environment.set("CODETERM_API_URL", api_url);
        environment.set("CODETERM_API_PORT", api_port);
        match api_token.filter(|token| !token.is_empty()) {
            Some(token) => environment.set("CODETERM_API_TOKEN", token),
            None => environment.remove("CODETERM_API_TOKEN"),
        }
        if let Some(path) = inherited_path {
            environment
                .variables
                .push((OsString::from("PATH"), path.to_os_string()));
        }
        if let Some(cli_binary) = cli_binary {
            if let Some(cli_dir) = crate::pty::manager::cli_dir_of(cli_binary.as_os_str()) {
                let inherited_path = inherited_path.unwrap_or_default();
                if let Some(path) =
                    crate::pty::manager::path_with_cli_dir_first(inherited_path, &cli_dir)
                {
                    environment.variables.retain(|(key, _)| key != "PATH");
                    environment.variables.push((OsString::from("PATH"), path));
                }
            }
        }
        environment
    }

    fn set(&mut self, key: &str, value: &str) {
        self.removed
            .retain(|removed| !removed.to_string_lossy().eq_ignore_ascii_case(key));
        self.variables
            .retain(|(name, _)| !name.to_string_lossy().eq_ignore_ascii_case(key));
        self.variables
            .push((OsString::from(key), OsString::from(value)));
    }

    fn remove(&mut self, key: &str) {
        self.variables
            .retain(|(name, _)| !name.to_string_lossy().eq_ignore_ascii_case(key));
        self.removed.push(OsString::from(key));
    }

    #[cfg(windows)]
    pub(super) fn is_empty(&self) -> bool {
        self.variables.is_empty() && self.removed.is_empty()
    }

    #[cfg(windows)]
    pub(super) fn windows_block(&self) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;

        let mut values: Vec<(OsString, OsString)> = std::env::vars_os().collect();
        for key in &self.removed {
            values.retain(|(name, _)| {
                !name
                    .to_string_lossy()
                    .eq_ignore_ascii_case(&key.to_string_lossy())
            });
        }
        for (key, value) in &self.variables {
            values.retain(|(name, _)| {
                !name
                    .to_string_lossy()
                    .eq_ignore_ascii_case(&key.to_string_lossy())
            });
            values.push((key.clone(), value.clone()));
        }
        values.sort_by(|(left, _), (right, _)| {
            left.to_string_lossy()
                .to_lowercase()
                .cmp(&right.to_string_lossy().to_lowercase())
        });

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

fn probe_path(inherited: Option<&OsStr>, defaults: &[&str]) -> Option<OsString> {
    let mut paths: Vec<_> = std::env::split_paths(inherited.unwrap_or_default()).collect();
    for directory in defaults {
        let directory = std::path::PathBuf::from(directory);
        if !paths.contains(&directory) {
            paths.push(directory);
        }
    }
    std::env::join_paths(paths).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_path_adds_homebrew_without_reordering_inherited_tools() {
        let inherited = std::env::join_paths(["/usr/bin", "/usr/local/bin"]).unwrap();
        let path = probe_path(Some(&inherited), &["/opt/homebrew/bin", "/usr/local/bin"]).unwrap();
        let entries: Vec<_> = std::env::split_paths(&path).collect();
        assert_eq!(
            entries,
            vec![
                std::path::PathBuf::from("/usr/bin"),
                std::path::PathBuf::from("/usr/local/bin"),
                std::path::PathBuf::from("/opt/homebrew/bin")
            ]
        );
    }

    #[test]
    fn sentinel_probe_environment_uses_instance_cli_and_auth() {
        let dir = tempfile::tempdir().unwrap();
        let instance_bin = dir.path().join("instance").join("bin");
        let cli = instance_bin.join(format!("codeterm{}", std::env::consts::EXE_SUFFIX));
        let stale_bin = dir.path().join("canonical").join("bin");
        let inherited = std::env::join_paths([stale_bin.clone()]).unwrap();
        let environment = Environment::for_instance(
            "C:/instance/socket",
            "http://127.0.0.1:7686",
            "7686",
            Some("instance-token"),
            Some(cli.as_path()),
            Some(&inherited),
        );

        let path = environment
            .variables
            .iter()
            .find(|(key, _)| key.to_string_lossy().eq_ignore_ascii_case("PATH"))
            .map(|(_, value)| value)
            .unwrap();
        let entries: Vec<_> = std::env::split_paths(path).collect();
        assert_eq!(entries.first(), Some(&instance_bin));
        assert!(entries.contains(&stale_bin));
        assert!(environment.variables.contains(&(
            OsString::from("CODETERM_API_URL"),
            OsString::from("http://127.0.0.1:7686")
        )));
        assert!(environment
            .variables
            .contains(&(OsString::from("CODETERM_API_PORT"), OsString::from("7686"))));
        assert!(environment.variables.contains(&(
            OsString::from("CODETERM_API_TOKEN"),
            OsString::from("instance-token")
        )));
        assert!(environment.variables.contains(&(
            OsString::from("CODETERM_SOCKET_PATH"),
            OsString::from("C:/instance/socket")
        )));
    }

    #[test]
    fn sentinel_probe_environment_removes_token_for_tokenless_instance() {
        let environment =
            Environment::for_instance("/socket", "http://127.0.0.1:7686", "7686", None, None, None);

        assert!(environment.removed.iter().any(|key| key
            .to_string_lossy()
            .eq_ignore_ascii_case("CODETERM_API_TOKEN")));
    }
}

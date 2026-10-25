use std::path::{Path, PathBuf};

pub(super) const REPAIR_COMMAND: &str = "domios system enable-autostart";

pub(super) struct InstalledSupervisor {
    candidates: Vec<PathBuf>,
    temporary_roots: Vec<PathBuf>,
}

impl InstalledSupervisor {
    pub(super) fn discover() -> Self {
        Self::new(
            std::env::consts::OS,
            dirs::home_dir().as_deref(),
            &super::installed_app_candidates(
                std::env::consts::OS,
                std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
                std::env::var_os("ProgramFiles").map(PathBuf::from),
                dirs::home_dir(),
            ),
            vec![std::env::temp_dir()],
        )
    }

    pub(super) fn new(
        platform: &str,
        home: Option<&Path>,
        apps: &[PathBuf],
        mut temporary_roots: Vec<PathBuf>,
    ) -> Self {
        temporary_roots.extend(
            [
                "/tmp",
                "/var/tmp",
                "/private/tmp",
                "/var/folders",
                "/private/var/folders",
            ]
            .map(PathBuf::from),
        );
        let names = if platform == "windows" {
            ["domios.exe", "codeterm.exe"]
        } else {
            ["domios", "codeterm"]
        };
        let mut roots = Vec::new();
        if let Some(home) = home {
            roots.push(home.join(".domios/bin"));
        }
        for app in apps {
            if let Some(parent) = app.parent() {
                match platform {
                    "macos" => {
                        if let Some(contents) = parent.parent() {
                            roots.push(contents.join("Resources/cli"));
                        }
                    }
                    "windows" => roots.push(parent.join("cli")),
                    "linux" => roots.push(parent.to_path_buf()),
                    _ => {}
                }
            }
        }
        Self {
            candidates: roots
                .into_iter()
                .flat_map(|root| names.map(|name| root.join(name)))
                .collect(),
            temporary_roots,
        }
    }

    fn is_temporary(&self, path: &Path) -> bool {
        let normalize = |path: &Path| {
            codeterm_paths::strip_verbatim_prefix(&path.to_string_lossy())
                .replace('\\', "/")
                .trim_end_matches('/')
                .to_lowercase()
        };
        let normalized = normalize(path);
        self.temporary_roots.iter().any(|root| {
            let root = normalize(root);
            normalized == root || normalized.starts_with(&format!("{root}/"))
        }) || normalized
            .split('/')
            .any(|part| part.starts_with("domios-worker-") || part.starts_with("codeterm-worker-"))
    }

    pub(super) fn resolve(
        &self,
        requested: Option<&Path>,
        canonical_file: impl Fn(&Path) -> Option<PathBuf>,
    ) -> Result<PathBuf, BrokenReason> {
        let installed: Vec<_> = self
            .candidates
            .iter()
            .filter_map(|candidate| {
                let resolved = canonical_file(candidate)?;
                (!self.is_temporary(candidate) && !self.is_temporary(&resolved)).then_some(resolved)
            })
            .collect();
        if let Some(requested) = requested {
            let resolved = canonical_file(requested).ok_or(BrokenReason::MissingTarget)?;
            if self.is_temporary(requested) || self.is_temporary(&resolved) {
                return Err(BrokenReason::TemporaryTarget);
            }
            return installed
                .into_iter()
                .find(|path| path == &resolved)
                .ok_or(BrokenReason::NotInstalledTarget);
        }
        installed
            .into_iter()
            .next()
            .ok_or(BrokenReason::MissingInstall)
    }
}

pub(super) fn canonical_file(path: &Path) -> Option<PathBuf> {
    path.is_file().then(|| path.canonicalize().ok()).flatten()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum BrokenReason {
    InvalidRegistration,
    MissingTarget,
    TemporaryTarget,
    NotInstalledTarget,
    MissingInstall,
}

#[derive(Debug, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub(super) enum AutostartState {
    Disabled,
    Enabled,
    Inactive,
    Broken {
        reason: BrokenReason,
        repair_command: &'static str,
    },
}

impl AutostartState {
    pub(super) fn assess(
        present: bool,
        enabled: bool,
        target: Option<&Path>,
        install: &InstalledSupervisor,
        canonical_file: impl Fn(&Path) -> Option<PathBuf>,
    ) -> Self {
        if !present {
            return Self::Disabled;
        }
        let validation = target
            .ok_or(BrokenReason::InvalidRegistration)
            .and_then(|target| install.resolve(Some(target), canonical_file));
        match validation {
            Err(reason) => Self::Broken {
                reason,
                repair_command: REPAIR_COMMAND,
            },
            Ok(_) if enabled => Self::Enabled,
            Ok(_) => Self::Inactive,
        }
    }

    pub(super) fn apply(&self, mut output: serde_json::Value) -> serde_json::Value {
        let fields = serde_json::to_value(self).unwrap_or_default();
        if let (Some(output), Some(fields)) = (output.as_object_mut(), fields.as_object()) {
            output.extend(fields.clone());
            output.insert("enabled".into(), (*self == Self::Enabled).into());
            if matches!(self, Self::Broken { .. }) {
                for field in ["loaded", "active"] {
                    if output.contains_key(field) {
                        output.insert(field.into(), false.into());
                    }
                }
            }
        }
        output
    }
}

#[cfg(test)]
mod tests;

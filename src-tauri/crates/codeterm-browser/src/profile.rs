use std::path::{Path, PathBuf};

pub const MAX_NAME_LEN: usize = 40;
pub const SESSIONS_DIR: &str = "sessions";
pub const PROFILES_DIR: &str = "profiles";
const WINDOWS_RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// `""` means the clean throwaway profile; otherwise a name safe as one directory on every OS.
pub fn sanitize_profile_name(raw: &str) -> String {
    let name: String = raw
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .take(MAX_NAME_LEN)
        .collect();
    if WINDOWS_RESERVED
        .iter()
        .any(|reserved| reserved.eq_ignore_ascii_case(&name))
    {
        format!("_{name}")
    } else {
        name
    }
}

pub fn owner_profile(scope: &str) -> String {
    format!("owner_{}", sanitize_profile_name(scope))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProfileKind {
    Clean { session_id: String },
    Named { name: String },
}

impl ProfileKind {
    pub fn from_request(raw: &str) -> Self {
        let name = sanitize_profile_name(raw);
        if name.is_empty() {
            Self::Clean {
                session_id: uuid::Uuid::new_v4().simple().to_string(),
            }
        } else {
            Self::Named { name }
        }
    }

    pub fn is_persistent(&self) -> bool {
        matches!(self, Self::Named { .. })
    }

    pub fn label(&self) -> &str {
        match self {
            Self::Clean { .. } => "clean",
            Self::Named { name } => name,
        }
    }
}

/// `<data_dir>/browsers/{sessions,profiles}/<id|name>`: one root so the orphan sweep can match it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileLayout {
    root: PathBuf,
}

impl ProfileLayout {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn under_data_dir(data_dir: &Path) -> Self {
        Self::new(data_dir.join("browsers"))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn sessions_dir(&self) -> PathBuf {
        self.root.join(SESSIONS_DIR)
    }

    pub fn profiles_dir(&self) -> PathBuf {
        self.root.join(PROFILES_DIR)
    }

    pub fn dir_for(&self, kind: &ProfileKind) -> PathBuf {
        match kind {
            ProfileKind::Clean { session_id } => self.sessions_dir().join(session_id),
            ProfileKind::Named { name } => self.profiles_dir().join(name),
        }
    }

    /// Named profile directories, sorted; entries that are not sanitizer-stable are skipped.
    pub fn list_named(&self) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(self.profiles_dir()) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
            .filter(|name| !name.is_empty() && sanitize_profile_name(name) == *name)
            .collect();
        names.sort();
        names
    }
}

pub mod instance;
pub mod lease_model;
mod management;
pub use management::validate_named_profile;
mod pool;
pub use pool::delete_named_profile;
pub use pool::ProfileLease;
pub(crate) use pool::SharedBrowser;

pub fn foreign_profile_error(dir: &Path, detail: &str) -> String {
    if [
        "SingletonLock",
        "ProcessSingleton",
        "Opening in existing browser session",
        "profile is already in use",
    ]
    .iter()
    .any(|marker| detail.contains(marker))
    {
        format!(
            "profile {} is already in use by a browser process Domios does not own: {detail}",
            dir.display()
        )
    } else {
        detail.to_string()
    }
}

#[cfg(test)]
mod tests;

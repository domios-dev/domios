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

pub mod lease_model;
mod pool;
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
            "profile {} is already in use by a browser process CodeTerm does not own: {detail}",
            dir.display()
        )
    } else {
        detail.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_profile_name_sanitizer_keeps_safe_chars() {
        assert_eq!(sanitize_profile_name("  work-Acme_2 "), "work-Acme_2");
        assert_eq!(
            sanitize_profile_name("../../etc/passwd"),
            "______etc_passwd"
        );
        assert_eq!(sanitize_profile_name("my profile.v2"), "my_profile_v2");
        assert_eq!(sanitize_profile_name("caf\u{e9} \u{1F600}"), "caf___");
        assert_eq!(sanitize_profile_name(""), "");
        assert_eq!(sanitize_profile_name("   "), "");
    }

    #[test]
    fn browser_profile_name_sanitizer_caps_length() {
        assert_eq!(sanitize_profile_name(&"a".repeat(100)).len(), MAX_NAME_LEN);
    }

    #[test]
    fn browser_profile_name_sanitizer_avoids_windows_device_names() {
        assert_eq!(sanitize_profile_name("nul"), "_nul");
        assert_eq!(sanitize_profile_name("COM1"), "_COM1");
        assert_eq!(sanitize_profile_name("console"), "console");
    }

    #[test]
    fn browser_profile_owner_scope_survives_reopen_and_keeps_owners_separate() {
        let layout = ProfileLayout::new(PathBuf::from("data/browsers"));
        let first = ProfileKind::from_request(&owner_profile("a123"));
        let reopened = ProfileKind::from_request(&owner_profile("a123"));
        let other = ProfileKind::from_request(&owner_profile("b456"));
        assert!(first.is_persistent());
        assert_eq!(layout.dir_for(&first), layout.dir_for(&reopened));
        assert_ne!(layout.dir_for(&first), layout.dir_for(&other));
    }

    #[test]
    fn browser_profile_kind_empty_name_is_clean_and_throwaway() {
        let clean = ProfileKind::from_request("  ");
        assert!(!clean.is_persistent());
        assert_eq!(clean.label(), "clean");
        let named = ProfileKind::from_request("work");
        assert!(named.is_persistent());
        assert_eq!(
            named,
            ProfileKind::Named {
                name: "work".into()
            }
        );
    }

    #[test]
    fn browser_profile_layout_places_clean_and_named_dirs_under_one_root() {
        let layout = ProfileLayout::under_data_dir(Path::new("/home/u/.codeterm"));
        let named = layout.dir_for(&ProfileKind::Named {
            name: "work".into(),
        });
        let clean = layout.dir_for(&ProfileKind::Clean {
            session_id: "abc".into(),
        });
        assert_eq!(
            named,
            PathBuf::from("/home/u/.codeterm/browsers/profiles/work")
        );
        assert_eq!(
            clean,
            PathBuf::from("/home/u/.codeterm/browsers/sessions/abc")
        );
    }

    #[test]
    fn browser_profile_layout_lists_only_stable_named_dirs() {
        let root = tempfile::tempdir().unwrap();
        let layout = ProfileLayout::new(root.path().to_path_buf());
        for name in ["work", "alpha", "has space"] {
            std::fs::create_dir_all(layout.profiles_dir().join(name)).unwrap();
        }
        std::fs::write(layout.profiles_dir().join("file"), b"x").unwrap();
        assert_eq!(
            layout.list_named(),
            vec!["alpha".to_string(), "work".to_string()]
        );
    }

    #[test]
    fn foreign_profile_lock_names_the_profile_and_external_owner() {
        let dir = Path::new("/profiles/work");
        let error = foreign_profile_error(dir, "ProcessSingleton: failed to create SingletonLock");
        assert!(error.contains(&dir.display().to_string()));
        assert!(error.contains("CodeTerm does not own"));
        assert_eq!(
            foreign_profile_error(dir, "binary missing"),
            "binary missing"
        );
    }
}

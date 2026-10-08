use parking_lot::Mutex;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

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

fn leases() -> &'static Mutex<HashSet<PathBuf>> {
    static LEASES: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
    LEASES.get_or_init(|| Mutex::new(HashSet::new()))
}

/// A second Chromium on a locked profile hands off to the first and exits, so one lease per dir.
#[derive(Debug)]
pub struct ProfileLease {
    dir: PathBuf,
}

impl ProfileLease {
    pub fn acquire(dir: PathBuf) -> Result<Self, String> {
        if !leases().lock().insert(dir.clone()) {
            return Err(format!(
                "profile {} is already in use by another browser",
                dir.display()
            ));
        }
        Ok(Self { dir })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn leased_dirs() -> HashSet<PathBuf> {
        leases().lock().clone()
    }
}

impl Drop for ProfileLease {
    fn drop(&mut self) {
        leases().lock().remove(&self.dir);
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
    fn browser_profile_lease_is_exclusive_until_dropped() {
        let dir = PathBuf::from(format!("/lease-test/{}", uuid::Uuid::new_v4().simple()));
        let lease = ProfileLease::acquire(dir.clone()).unwrap();
        assert!(ProfileLease::acquire(dir.clone()).is_err());
        assert!(ProfileLease::leased_dirs().contains(&dir));
        drop(lease);
        assert!(ProfileLease::acquire(dir).is_ok());
    }
}

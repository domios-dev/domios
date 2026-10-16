use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct InstallationId(String);

impl InstallationId {
    pub fn parse(value: String) -> io::Result<Self> {
        if value.is_empty()
            || value.chars().count() > 128
            || value.chars().any(|c| c < ' ' || c == '\u{7f}')
            || value.contains(['/', '\\'])
            || value == "."
            || value == ".."
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid installation id",
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn generate() -> Self {
        let mut bytes = [0u8; 16];
        rand::rngs::OsRng.fill_bytes(&mut bytes);
        bytes[6] = (bytes[6] & 0x0f) | 0x40;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        Self(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
    }
}

impl<'de> Deserialize<'de> for InstallationId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::parse(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

pub trait InstallationStore {
    fn load_or_create(&self) -> io::Result<InstallationId>;
}

pub struct FileInstallationStore {
    path: PathBuf,
}

impl FileInstallationStore {
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            path: data_dir.join("state/account/installation.json"),
        }
    }
}

impl FileInstallationStore {
    fn load_or_create_with(
        &self,
        link: impl FnOnce(&Path, &Path) -> io::Result<()>,
    ) -> io::Result<InstallationId> {
        match fs::read(&self.path) {
            Ok(bytes) => return serde_json::from_slice(&bytes).map_err(io::Error::other),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let id = InstallationId::generate();
        let bytes = serde_json::to_vec(&id).map_err(io::Error::other)?;
        // Publish complete bytes atomically; a concurrent creator's existing identity always wins.
        let temporary = self
            .path
            .with_extension(format!("{}.tmp", InstallationId::generate().as_str()));
        fs::write(&temporary, &bytes)?;
        let published = link(&temporary, &self.path);
        fs::remove_file(&temporary)?;
        match published {
            Ok(()) => Ok(id),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                serde_json::from_slice(&fs::read(&self.path)?).map_err(io::Error::other)
            }
            Err(_) => match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&self.path)
            {
                Ok(mut file) => {
                    file.write_all(&bytes)?;
                    Ok(id)
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    serde_json::from_slice(&fs::read(&self.path)?).map_err(io::Error::other)
                }
                Err(error) => Err(error),
            },
        }
    }
}
impl InstallationStore for FileInstallationStore {
    fn load_or_create(&self) -> io::Result<InstallationId> {
        self.load_or_create_with(|from, to| fs::hard_link(from, to))
    }
}

impl crate::Account {
    pub fn installation_id(store: &dyn InstallationStore) -> io::Result<InstallationId> {
        store.load_or_create()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "codeterm-installation-{}",
                InstallationId::generate().as_str()
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn created_once_and_stable_across_reload() {
        let dir = TempDir::new();
        let store = FileInstallationStore::new(dir.0.clone());
        let first = crate::Account::installation_id(&store).unwrap();
        assert_eq!(first.as_str().len(), 32);
        assert_eq!(&first.as_str()[12..13], "4");
        assert!(matches!(&first.as_str()[16..17], "8" | "9" | "a" | "b"));
        let bytes = fs::read(&store.path).unwrap();
        let reloaded = FileInstallationStore::new(dir.0.clone());
        assert_eq!(first, reloaded.load_or_create().unwrap());
        assert_eq!(bytes, fs::read(&store.path).unwrap());
    }

    #[test]
    fn unsupported_hard_links_fall_back_to_exclusive_creation() {
        let dir = TempDir::new();
        let store = FileInstallationStore::new(dir.0.clone());
        let first = store
            .load_or_create_with(|_, _| Err(io::ErrorKind::Unsupported.into()))
            .unwrap();
        assert_eq!(store.load_or_create().unwrap(), first);
        assert_eq!(
            serde_json::from_slice::<InstallationId>(&fs::read(&store.path).unwrap()).unwrap(),
            first
        );
    }

    #[test]
    fn fallback_never_overwrites_a_concurrent_creator() {
        let dir = TempDir::new();
        let store = FileInstallationStore::new(dir.0.clone());
        let winner = InstallationId::parse("concurrent-winner".into()).unwrap();
        let result = store
            .load_or_create_with(|_, destination| {
                fs::write(destination, serde_json::to_vec(&winner).unwrap())?;
                Err(io::ErrorKind::Unsupported.into())
            })
            .unwrap();
        assert_eq!(result, winner);
        assert_eq!(store.load_or_create().unwrap(), winner);
    }

    #[test]
    fn invalid_existing_identity_is_never_replaced() {
        let dir = TempDir::new();
        let store = FileInstallationStore::new(dir.0.clone());
        store.load_or_create().unwrap();
        for value in [
            "",
            ".",
            "..",
            "a/b",
            "a\\b",
            "a\n",
            "\u{7f}",
            &"x".repeat(129),
        ] {
            let bytes = serde_json::to_vec(value).unwrap();
            fs::write(&store.path, &bytes).unwrap();
            assert!(store.load_or_create().is_err());
            assert_eq!(fs::read(&store.path).unwrap(), bytes);
        }
        assert!(InstallationId::parse("😀".repeat(128)).is_ok());
    }
}

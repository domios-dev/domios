use crate::Result;
use codeterm_entitlements::{Binding, DetachedGrant, EntitlementAuthority, Refusal, VerifiedGrant};
use std::{fs, path::PathBuf};

pub trait GrantStore: Send + Sync {
    fn load(&self) -> Result<Option<serde_json::Value>>;
    fn save(&self, grant: &serde_json::Value) -> Result<()>;
    fn clear(&self) -> Result<()>;
}

pub struct FileGrantStore {
    path: PathBuf,
}
impl FileGrantStore {
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            path: data_dir.join("state/account/grant.json"),
        }
    }
}
impl GrantStore for FileGrantStore {
    fn load(&self) -> Result<Option<serde_json::Value>> {
        match fs::read(&self.path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|error| error.to_string()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    }
    fn save(&self, grant: &serde_json::Value) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let bytes = serde_json::to_vec(grant).map_err(|e| e.to_string())?;
        fs::write(&self.path, bytes).map_err(|e| e.to_string())
    }
    fn clear(&self) -> Result<()> {
        match fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.to_string()),
        }
    }
}

pub(crate) fn authenticate(
    raw: &serde_json::Value,
    authority: &dyn EntitlementAuthority,
    uid: &str,
    installation_id: &str,
    cached: Option<&VerifiedGrant>,
    now: u64,
) -> std::result::Result<VerifiedGrant, Refusal> {
    let detached: DetachedGrant =
        serde_json::from_value(raw.clone()).map_err(|_| Refusal::Malformed)?;
    authority
        .verify(
            &detached,
            &Binding {
                uid,
                installation_id,
            },
            cached,
            now,
        )
        .map(|accepted| accepted.grant)
        .map_err(|rejected| rejected.refusal)
}

use crate::Result;
use codeterm_entitlements::{Binding, DetachedGrant, EntitlementAuthority, Refusal, VerifiedGrant};
use std::{fs, path::PathBuf};

pub trait GrantStore: Send + Sync {
    fn load(&self) -> Result<Option<serde_json::Value>>;
    fn save(&self, grant: &serde_json::Value) -> Result<()>;
    fn clear(&self) -> Result<()>;
    /// Whether the account was paid at the last definite answer; it never grants, only delays locks.
    fn load_hint(&self) -> Result<bool>;
    fn save_hint(&self, paid: bool) -> Result<()>;
}

pub struct FileGrantStore {
    path: PathBuf,
    hint: PathBuf,
}
impl FileGrantStore {
    pub fn new(data_dir: PathBuf) -> Self {
        let dir = data_dir.join("state/account");
        Self {
            path: dir.join("grant.json"),
            hint: dir.join("plan_hint.json"),
        }
    }
}

fn write_private(path: &std::path::Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(path).map_err(|e| e.to_string())?;
    std::io::Write::write_all(&mut file, bytes).map_err(|e| e.to_string())
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
        let bytes = serde_json::to_vec(grant).map_err(|e| e.to_string())?;
        write_private(&self.path, &bytes)
    }
    fn load_hint(&self) -> Result<bool> {
        match fs::read(&self.hint) {
            Ok(bytes) => Ok(serde_json::from_slice::<serde_json::Value>(&bytes)
                .ok()
                .and_then(|value| value["paid"].as_bool())
                .unwrap_or(false)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error.to_string()),
        }
    }
    fn save_hint(&self, paid: bool) -> Result<()> {
        write_private(
            &self.hint,
            serde_json::json!({ "paid": paid }).to_string().as_bytes(),
        )
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

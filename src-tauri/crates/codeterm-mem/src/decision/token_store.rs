use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;

#[cfg(windows)]
mod windows;

#[derive(Clone)]
pub struct ApiToken(String);

impl ApiToken {
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ApiToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApiToken(<redacted>)")
    }
}

const RESERVED_FILES: [&str; 3] = ["spend.json", "calls.json", "launch-origins.json"];

#[derive(Debug, Clone)]
pub struct TokenStore {
    base_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenStatus {
    pub present: bool,
    pub path: PathBuf,
    pub mode_ok: Option<bool>,
}

#[cfg(windows)]
pub fn restrict_directory_permissions(path: &std::path::Path) -> io::Result<()> {
    windows::restrict_dir(path)
}

impl TokenStore {
    pub fn new(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    pub fn adapters(&self) -> io::Result<Vec<String>> {
        let entries = match fs::read_dir(&self.base_dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let mut ids = Vec::new();
        for entry in entries {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let path = entry.path();
            if path
                .file_name()
                .is_some_and(|name| RESERVED_FILES.iter().any(|reserved| name == *reserved))
                || path.extension().is_none_or(|extension| extension != "json")
            {
                continue;
            }
            if let Some(id) = path.file_stem().and_then(|id| id.to_str()) {
                ids.push(id.into());
            }
        }
        Ok(ids)
    }

    pub fn path(&self, adapter_id: &str) -> PathBuf {
        let safe: String = adapter_id
            .bytes()
            .map(|b| {
                if b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' {
                    b as char
                } else {
                    '-'
                }
            })
            .collect();
        let safe = safe.trim_matches('-');
        self.base_dir.join(format!(
            "{}.json",
            if safe.is_empty() { "adapter" } else { safe }
        ))
    }

    fn token_path(&self, adapter_id: &str) -> io::Result<PathBuf> {
        let path = self.path(adapter_id);
        if path
            .file_name()
            .is_some_and(|name| RESERVED_FILES.iter().any(|reserved| name == *reserved))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "reserved token file name",
            ));
        }
        Ok(path)
    }

    pub fn set(&self, adapter_id: &str, token: &str) -> io::Result<()> {
        let token = token.trim();
        if token.is_empty() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "empty token"));
        }
        fs::create_dir_all(&self.base_dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&self.base_dir, fs::Permissions::from_mode(0o700))?;
        }
        #[cfg(windows)]
        windows::restrict_dir(&self.base_dir)?;
        let target = self.token_path(adapter_id)?;
        let temp = target.with_file_name(format!(
            ".{}.{}.tmp",
            target.file_name().unwrap().to_string_lossy(),
            uuid::Uuid::new_v4()
        ));
        let result = (|| {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&temp)?;
            serde_json::to_writer(&mut file, &serde_json::json!({ "token": token })).map_err(
                |_| io::Error::new(io::ErrorKind::InvalidData, "token serialization failed"),
            )?;
            file.flush()?;
            file.sync_all()?;
            #[cfg(windows)]
            windows::restrict(&temp)?;
            fs::rename(&temp, &target)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result
    }

    pub fn remove_transaction<T>(
        &self,
        adapter_id: &str,
        commit: impl FnOnce() -> io::Result<T>,
    ) -> io::Result<T> {
        let target = self.token_path(adapter_id)?;
        let staged = self
            .base_dir
            .join(format!(".removed-{}.tmp", uuid::Uuid::new_v4()));
        let moved = match fs::rename(&target, &staged) {
            Ok(()) => true,
            Err(error) if error.kind() == io::ErrorKind::NotFound => false,
            Err(error) => return Err(error),
        };
        match commit() {
            Ok(value) => {
                if moved {
                    fs::remove_file(staged)?;
                }
                Ok(value)
            }
            Err(error) => {
                if moved {
                    fs::rename(staged, target)?;
                }
                Err(error)
            }
        }
    }

    pub fn clear(&self, adapter_id: &str) -> io::Result<bool> {
        match fs::remove_file(self.token_path(adapter_id)?) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }

    pub fn status(&self, adapter_id: &str) -> io::Result<TokenStatus> {
        let path = self.token_path(adapter_id)?;
        let metadata = match fs::metadata(&path) {
            Ok(metadata) => Some(metadata),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        #[cfg(unix)]
        let mode_ok = {
            use std::os::unix::fs::PermissionsExt;
            metadata
                .as_ref()
                .map(|m| m.permissions().mode() & 0o777 == 0o600)
        };
        #[cfg(windows)]
        let mode_ok = windows::mode_ok(&path).ok();
        #[cfg(all(not(unix), not(windows)))]
        let mode_ok = None;
        Ok(TokenStatus {
            present: metadata.is_some(),
            path,
            mode_ok,
        })
    }

    pub fn load(&self, adapter_id: &str) -> io::Result<Option<ApiToken>> {
        let bytes = match fs::read(self.token_path(adapter_id)?) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let value: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid token file"))?;
        let token = value
            .get("token")
            .and_then(|v| v.as_str())
            .filter(|v| !v.trim().is_empty())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid token file"))?;
        Ok(Some(ApiToken(token.to_owned())))
    }
}

#[cfg(test)]
mod tests;

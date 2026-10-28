use async_trait::async_trait;
use codeterm_neighbors::{
    crypto::sha256_hex,
    permissions::Permission,
    receive::CursorStore,
    wire::{hex_id, Cursor},
    Error, Result,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Clone)]
pub(super) struct Metadata {
    root: PathBuf,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct InitialContext {
    pub gateway_tab: String,
    pub digest: String,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Settings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cleanup_pending: bool,
    pub bindings: BTreeMap<String, String>,
    pub permissions: BTreeMap<String, Permission>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub initial_contexts: BTreeMap<String, InitialContext>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub notifications: BTreeMap<String, super::NeighborNotify>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeSet::is_empty")]
    pub paused: std::collections::BTreeSet<String>,
}

impl Settings {
    pub(super) fn initial_context_digest(&self, friendship: &str) -> Option<&str> {
        let context = self.initial_contexts.get(friendship)?;
        (self.bindings.get(friendship) == Some(&context.gateway_tab))
            .then_some(context.digest.as_str())
    }
}

#[async_trait]
impl codeterm_neighbors::keys::EnrollmentState for Metadata {
    async fn enable(&self) -> Result<()> {
        let mut settings = self.settings().await?;
        settings.enabled = true;
        self.save_settings(settings).await
    }
}

impl Metadata {
    pub(super) fn friend_folder(&self, friendship: &str) -> Result<PathBuf> {
        if !hex_id(friendship, 64) {
            return Err(Error::InvalidEnvelope);
        }
        Ok(self.root.join("friends").join(friendship))
    }

    pub fn new(root: PathBuf, uid: &str) -> Self {
        Self {
            root: root.join("neighbors").join(sha256_hex(uid.as_bytes())),
        }
    }

    pub(super) async fn read<T: DeserializeOwned + Send + 'static>(
        &self,
        name: String,
    ) -> Result<Option<T>> {
        let path = self.root.join(name);
        tokio::task::spawn_blocking(move || {
            let metadata = match std::fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(_) => return Err(Error::CursorStore),
            };
            if !metadata.is_file()
                || metadata.file_type().is_symlink()
                || metadata.len() > 1024 * 1024
                || !codeterm_util::private_file::owner_only(&path)
                    .map_err(|_| Error::CursorStore)?
            {
                return Err(Error::CursorStore);
            }
            let bytes = std::fs::read(path).map_err(|_| Error::CursorStore)?;
            serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|_| Error::CursorStore)
        })
        .await
        .map_err(|_| Error::CursorStore)?
    }

    pub(super) async fn write<T: Serialize + Send + 'static>(
        &self,
        name: String,
        value: T,
    ) -> Result<()> {
        let path = self.root.join(name);
        tokio::task::spawn_blocking(move || {
            let bytes = serde_json::to_vec(&value).map_err(|_| Error::CursorStore)?;
            if bytes.len() > 1024 * 1024 {
                return Err(Error::CursorStore);
            }
            codeterm_util::private_file::write_atomic(&path, &bytes).map_err(|_| Error::CursorStore)
        })
        .await
        .map_err(|_| Error::CursorStore)?
    }

    pub async fn settings(&self) -> Result<Settings> {
        Ok(self.read("settings.json".into()).await?.unwrap_or_default())
    }

    pub async fn save_settings(&self, settings: Settings) -> Result<()> {
        self.write("settings.json".into(), settings).await
    }
}

#[async_trait]
impl CursorStore for Metadata {
    async fn load(&self, friendship: &str) -> Result<Option<Cursor>> {
        if !hex_id(friendship, 64) {
            return Err(Error::InvalidEnvelope);
        }
        let cursor: Option<Cursor> = self.read(format!("cursor-{friendship}.json")).await?;
        if let Some(cursor) = &cursor {
            cursor.validate().map_err(|_| Error::CursorStore)?;
        }
        Ok(cursor)
    }
    async fn save(&self, friendship: &str, cursor: &Cursor) -> Result<()> {
        if !hex_id(friendship, 64) {
            return Err(Error::InvalidEnvelope);
        }
        cursor.validate()?;
        self.write(format!("cursor-{friendship}.json"), cursor.clone())
            .await
    }
}

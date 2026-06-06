//! Bitwarden cipher types + folder/item helpers.

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::secrets::{SaveRequest, SecretError, SecretRef};

use super::store::{unwrap_data_envelope, BitwardenStore};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct BwCipher {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub cipher_type: i64,
    #[serde(default, rename = "folderId")]
    pub folder_id: Option<String>,
    #[serde(default, rename = "organizationId")]
    pub organization_id: Option<String>,
    #[serde(default, rename = "collectionIds")]
    pub collection_ids: Vec<String>,
    #[serde(default, rename = "revisionDate")]
    pub revision_date: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub login: Option<BwLogin>,
}

impl BwCipher {
    pub(super) fn into_ref(self) -> SecretRef {
        SecretRef {
            item_id: self.id,
            name: self.name,
            folder: self.folder_id, // raw id; UI resolves to name lazily
            organization: self.organization_id,
            collection: self.collection_ids.into_iter().next(),
            last_modified: self.revision_date.and_then(parse_iso8601_to_ms),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct BwLogin {
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
}

pub(super) fn build_login_cipher(
    req: &SaveRequest,
    existing_id: Option<&str>,
    folder_id: Option<&str>,
) -> serde_json::Value {
    json!({
        "id": existing_id,
        "type": 1,
        "name": req.name,
        "notes": req.notes,
        "folderId": folder_id,
        "organizationId": req.organization,
        "collectionIds": req.collection.clone().map(|c| vec![c]).unwrap_or_default(),
        "favorite": false,
        "login": {
            "username": null,
            "password": req.value.expose_secret(),
            "totp": null,
            "uris": []
        }
    })
}

pub(super) fn parse_iso8601_to_ms(s: String) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(&s)
        .ok()
        .map(|dt| dt.timestamp_millis())
}

impl BitwardenStore {
    pub(super) async fn fetch_item(&self, name_or_id: &str) -> Result<BwCipher, SecretError> {
        let data = self
            .run_with_session(vec!["get", "item", name_or_id], None)
            .await?;
        serde_json::from_value(unwrap_data_envelope(data)).map_err(|e| SecretError::Backend {
            message: format!("parse item: {}", e),
        })
    }

    pub(super) async fn find_by_name(&self, name: &str) -> Result<BwCipher, SecretError> {
        let data = self
            .run_with_session(vec!["list", "items", "--search", name], None)
            .await?;
        let items: Vec<BwCipher> =
            serde_json::from_value(unwrap_data_envelope(data)).map_err(|e| {
                SecretError::Backend {
                    message: format!("parse list: {}", e),
                }
            })?;
        let exact: Vec<BwCipher> = items
            .into_iter()
            .filter(|c| c.cipher_type == 1 && c.name.eq_ignore_ascii_case(name))
            .collect();
        match exact.len() {
            0 => Err(SecretError::NotFound),
            1 => Ok(exact.into_iter().next().unwrap()),
            _ => Err(SecretError::Ambiguous {
                candidates: exact.into_iter().map(|c| c.id).collect(),
            }),
        }
    }

    pub(super) async fn resolve_folder_id(
        &self,
        name: &str,
    ) -> Result<Option<String>, SecretError> {
        if name.is_empty() {
            return Ok(None);
        }
        let data = self.run_with_session(vec!["list", "folders"], None).await?;
        let folders: Vec<serde_json::Value> = serde_json::from_value(unwrap_data_envelope(data))
            .map_err(|e| SecretError::Backend {
                message: format!("parse folders: {}", e),
            })?;
        Ok(folders
            .into_iter()
            .find(|f| {
                f.get("name")
                    .and_then(|n| n.as_str())
                    .map(|n| n.eq_ignore_ascii_case(name))
                    .unwrap_or(false)
            })
            .and_then(|f| f.get("id").and_then(|i| i.as_str()).map(String::from)))
    }

    pub(super) async fn resolve_or_create_folder(&self, name: &str) -> Result<String, SecretError> {
        if let Some(id) = self.resolve_folder_id(name).await? {
            return Ok(id);
        }
        let body = json!({ "name": name });
        let encoded = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            body.to_string().as_bytes(),
        );
        let data = self
            .run_with_session(vec!["create", "folder", &encoded], None)
            .await?;
        let inner = unwrap_data_envelope(data);
        inner
            .get("id")
            .and_then(|i| i.as_str())
            .map(String::from)
            .ok_or_else(|| SecretError::Backend {
                message: "create folder did not return id".into(),
            })
    }
}

//! `impl SecretStore for BitwardenStore` — all CRUD trait methods.

use std::collections::HashMap;

use async_trait::async_trait;

use crate::secrets::{
    validate_name, CollectionRef, ListFilter, OrganizationRef, SaveRequest, SecretError, SecretRef,
    SecretStore, SecretString, SecretValue, StoreStatus, UnlockCredentials,
};

use super::cipher::{build_login_cipher, BwCipher};
use super::store::{is_valid_http_url, unwrap_data_envelope, BitwardenStore};
use super::{
    BW_KEYRING_API_CLIENT_ID, BW_KEYRING_API_CLIENT_SECRET, BW_KEYRING_LOGIN_EMAIL,
    BW_KEYRING_SESSION,
};

#[async_trait]
impl SecretStore for BitwardenStore {
    fn id(&self) -> &'static str {
        "bitwarden"
    }

    async fn status(&self) -> StoreStatus {
        if let Some(reason) = &self.unavailable_reason {
            return StoreStatus::Unavailable {
                reason: reason.clone(),
            };
        }
        let endpoint = Some(self.server_url.lock().clone());
        match self.bw_status().await {
            Ok(s) => match s.status.as_str() {
                "unlocked" => {
                    if let Some(email) = s.user_email.clone() {
                        *self.user.lock() = Some(email);
                    }
                    StoreStatus::Unlocked {
                        user: self.user.lock().clone(),
                        transient: !self.keychain.is_available(),
                        endpoint: s.server_url.or(endpoint),
                    }
                }
                "locked" => StoreStatus::Locked { endpoint },
                _ => StoreStatus::LoggedOut { endpoint },
            },
            Err(SecretError::Backend { message }) => StoreStatus::Unavailable { reason: message },
            Err(SecretError::Locked) => StoreStatus::Locked { endpoint },
            Err(SecretError::LoggedOut) => StoreStatus::LoggedOut { endpoint },
            Err(other) => StoreStatus::Unavailable {
                reason: other.to_string(),
            },
        }
    }

    async fn unlock(&self, creds: UnlockCredentials) -> Result<(), SecretError> {
        self.require_available()?;
        if creds.master_password.is_empty() && creds.api_key_client_id.is_none() {
            return Err(SecretError::BadRequest {
                message: "master password (or API-key creds) required".into(),
            });
        }

        // Validate server URL — guard against malformed config that would otherwise be passed verbatim to `bw config server`.
        let server = self.server_url.lock().clone();
        if !is_valid_http_url(&server) {
            return Err(SecretError::BadRequest {
                message: format!("invalid bitwarden server URL: {}", server),
            });
        }

        let status = self.bw_status().await.ok();
        let is_logged_in = status
            .as_ref()
            .map(|s| s.status != "unauthenticated")
            .unwrap_or(false);

        // Point bw at the configured server BEFORE any login. Safe to re-run on every unlock — bw treats it as idempotent. If the URL differs from a
        // logged-in server, force a logout first so login talks to the right host.
        let current_server = status
            .as_ref()
            .and_then(|s| s.server_url.clone())
            .unwrap_or_default();
        if is_logged_in && !current_server.is_empty() && current_server != server {
            log::info!(
                "[secrets/bw] server URL changed ({} -> {}), forcing logout",
                current_server,
                server
            );
            let _ = self
                .run_no_session(vec!["logout"], None, HashMap::new())
                .await;
            *self.session.write().await = None;
            let _ = self.keychain.delete(BW_KEYRING_SESSION);
        }
        // Apply the server config only when it changes. bw refuses to update the server while logged in to it ("Logout required before server config
        // update"), so re-running config server with the same URL is both pointless AND breaks the common no-op login flow.
        if current_server != server {
            if let Err(e) = self
                .run_no_session(vec!["config", "server", &server], None, HashMap::new())
                .await
            {
                log::warn!("[secrets/bw] config server '{}' failed: {}", server, e);
                return Err(SecretError::Backend {
                    message: format!("could not point bw at {}: {}", server, e),
                });
            }
        }

        // Re-read login state after potential logout above.
        let status = self.bw_status().await.ok();
        let is_logged_in = status
            .as_ref()
            .map(|s| s.status != "unauthenticated")
            .unwrap_or(false);

        if !is_logged_in {
            // Need to log in first. "You are already logged in" is bw's way of saying "skip the login step" — treat as soft success so we proceed to unlock with the existing session.
            let login_result: Result<serde_json::Value, SecretError> =
                if let (Some(client_id), Some(client_secret)) =
                    (&creds.api_key_client_id, &creds.api_key_client_secret)
                {
                    let mut env = HashMap::new();
                    env.insert("BW_CLIENTID".into(), client_id.expose_secret().into());
                    env.insert(
                        "BW_CLIENTSECRET".into(),
                        client_secret.expose_secret().into(),
                    );
                    let r = self
                        .run_no_session(vec!["login", "--apikey"], None, env)
                        .await;
                    if r.is_ok() {
                        let _ = self
                            .keychain
                            .set(BW_KEYRING_API_CLIENT_ID, client_id.expose_secret());
                        let _ = self
                            .keychain
                            .set(BW_KEYRING_API_CLIENT_SECRET, client_secret.expose_secret());
                    }
                    r
                } else if let Some(email) = creds.email.as_deref() {
                    let mut env = HashMap::new();
                    env.insert(
                        "BW_PASSWORD".into(),
                        creds.master_password.expose_secret().to_string(),
                    );
                    let mut args: Vec<&str> = vec!["login", email, "--passwordenv", "BW_PASSWORD"];
                    if let Some(tok) = creds.two_factor_token.as_ref() {
                        args.push("--method");
                        args.push("0");
                        args.push("--code");
                        args.push(tok.expose_secret());
                    }
                    self.run_no_session(args, None, env).await
                } else {
                    return Err(SecretError::BadRequest {
                        message: "either email + master_password or api_key_* required".into(),
                    });
                };

            if let Err(SecretError::Backend { message }) = &login_result {
                if message.to_lowercase().contains("already logged in") {
                    log::info!("[secrets/bw] login skipped — bw reports already logged in");
                } else {
                    return Err(SecretError::Backend {
                        message: message.clone(),
                    });
                }
            } else if let Err(other) = login_result {
                return Err(other);
            }
        }

        // Now unlock to get the session token.
        let mut env = HashMap::new();
        env.insert(
            "BW_PASSWORD".into(),
            creds.master_password.expose_secret().to_string(),
        );
        let unlock_data = self
            .run_no_session(
                vec!["unlock", "--passwordenv", "BW_PASSWORD", "--raw"],
                None,
                env,
            )
            .await?;

        // bw unlock --raw output shape varies by version: modern: { "object": "unlock", "raw": "<token>", ... } modern wrapped: { "template": { "raw":
        // "<token>" } } legacy: just the raw token string at the top level
        let token_owned: String = unlock_data
            .as_str()
            .map(|s| s.to_string())
            .or_else(|| {
                unlock_data
                    .get("raw")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
            .or_else(|| {
                unlock_data
                    .get("template")
                    .and_then(|t| t.get("raw"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
            .ok_or_else(|| SecretError::Backend {
                message: format!(
                    "bw unlock returned unexpected payload shape: {}",
                    unlock_data
                ),
            })?;
        let token = token_owned.trim().to_string();
        if token.is_empty() {
            return Err(SecretError::Backend {
                message: "bw unlock returned empty session".into(),
            });
        }

        *self.session.write().await = Some(SecretString::from(token.clone()));
        let _ = self.keychain.set(BW_KEYRING_SESSION, &token);
        // Always persist master pw to file (mode 0600, ~/.codeterm/secrets/) for JIT auto-unlock. Skipped only when the user signed in via API key (no
        // master pw to save).
        if creds.api_key_client_id.is_none() {
            let _ = super::write_master_password_file(creds.master_password.expose_secret());
            if let Some(email) = creds.email.as_deref() {
                let _ = self.keychain.set(BW_KEYRING_LOGIN_EMAIL, email);
            }
        }
        Ok(())
    }

    async fn lock(&self) -> Result<(), SecretError> {
        self.require_available()?;
        let _ = self
            .run_no_session(vec!["lock"], None, HashMap::new())
            .await;
        *self.session.write().await = None;
        let _ = self.keychain.delete(BW_KEYRING_SESSION);
        // Master password stays — unlock without prompting again.
        Ok(())
    }

    async fn logout(&self) -> Result<(), SecretError> {
        self.require_available()?;
        let _ = self
            .run_no_session(vec!["logout"], None, HashMap::new())
            .await;
        *self.session.write().await = None;
        *self.user.lock() = None;
        let _ = self.keychain.delete(BW_KEYRING_SESSION);
        let _ = self.keychain.delete(BW_KEYRING_API_CLIENT_ID);
        let _ = self.keychain.delete(BW_KEYRING_API_CLIENT_SECRET);
        let _ = self.keychain.delete(BW_KEYRING_LOGIN_EMAIL);
        super::delete_master_password_file();
        Ok(())
    }

    async fn save(&self, req: SaveRequest) -> Result<SecretRef, SecretError> {
        validate_name(&req.name)?;
        if req.value.is_empty() {
            return Err(SecretError::BadRequest {
                message: "value must not be empty".into(),
            });
        }

        // If overwriting, look up the existing item id first; bw create will happily insert a duplicate-name item, which we never want.
        let existing = self.find_by_name(&req.name).await.ok();
        if existing.is_some() && !req.overwrite {
            return Err(SecretError::AlreadyExists);
        }

        let folder_id = match &req.folder {
            Some(f) => Some(self.resolve_or_create_folder(f).await?),
            None => None,
        };
        let cipher = build_login_cipher(
            &req,
            existing.as_ref().map(|c| c.id.as_str()),
            folder_id.as_deref(),
        );
        let json_body = serde_json::to_string(&cipher).map_err(|e| SecretError::Backend {
            message: format!("encode cipher: {}", e),
        })?;
        let encoded = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            json_body.as_bytes(),
        );

        let data = if let Some(c) = &existing {
            self.run_with_session(vec!["edit", "item", &c.id, &encoded], None)
                .await?
        } else {
            self.run_with_session(vec!["create", "item", &encoded], None)
                .await?
        };

        // Sync so subsequent list/get see the change without a manual sync.
        let _ = self.run_with_session(vec!["sync"], None).await;

        let saved: BwCipher = serde_json::from_value(unwrap_data_envelope(data)).map_err(|e| {
            SecretError::Backend {
                message: format!("parse saved item: {}", e),
            }
        })?;
        Ok(saved.into_ref())
    }

    async fn get(&self, name_or_id: &str) -> Result<SecretValue, SecretError> {
        validate_name(name_or_id).ok();
        let item = self.fetch_item(name_or_id).await?;
        let value = item
            .login
            .as_ref()
            .and_then(|l| l.password.clone())
            .ok_or_else(|| SecretError::Backend {
                message: "item has no login.password field".into(),
            })?;
        Ok(SecretValue {
            item_id: item.id.clone(),
            name: item.name.clone(),
            value: SecretString::from(value),
            notes: item.notes.clone(),
        })
    }

    async fn list(&self, filter: ListFilter) -> Result<Vec<SecretRef>, SecretError> {
        let mut args: Vec<String> = vec!["list".into(), "items".into()];
        if let Some(org) = &filter.organization {
            args.push("--organizationid".into());
            args.push(org.clone());
        }
        if let Some(coll) = &filter.collection {
            args.push("--collectionid".into());
            args.push(coll.clone());
        }
        let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        let data = self.run_with_session(arg_refs, None).await?;
        let mut items: Vec<BwCipher> =
            serde_json::from_value(unwrap_data_envelope(data)).map_err(|e| {
                SecretError::Backend {
                    message: format!("parse list: {}", e),
                }
            })?;
        if let Some(folder) = &filter.folder {
            // Bitwarden references folders by id; we accept the human name.
            let folder_id = self.resolve_folder_id(folder).await?;
            items.retain(|c| c.folder_id.as_deref() == folder_id.as_deref());
        }
        items.retain(|c| c.cipher_type == 1); // logins only
        items.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        if let Some(limit) = filter.limit {
            items.truncate(limit);
        }
        Ok(items.into_iter().map(|c| c.into_ref()).collect())
    }

    async fn search(&self, query: &str) -> Result<Vec<SecretRef>, SecretError> {
        let q = query.trim();
        if q.is_empty() {
            return Err(SecretError::BadRequest {
                message: "query must not be empty".into(),
            });
        }
        let data = self
            .run_with_session(vec!["list", "items", "--search", q], None)
            .await?;
        let items: Vec<BwCipher> =
            serde_json::from_value(unwrap_data_envelope(data)).map_err(|e| {
                SecretError::Backend {
                    message: format!("parse search: {}", e),
                }
            })?;
        Ok(items
            .into_iter()
            .filter(|c| c.cipher_type == 1)
            .map(|c| c.into_ref())
            .collect())
    }

    async fn delete(&self, name_or_id: &str) -> Result<(), SecretError> {
        let item = self.fetch_item(name_or_id).await?;
        self.run_with_session(vec!["delete", "item", &item.id, "--permanent"], None)
            .await?;
        let _ = self.run_with_session(vec!["sync"], None).await;
        Ok(())
    }

    async fn organizations(&self) -> Result<Vec<OrganizationRef>, SecretError> {
        let data = self
            .run_with_session(vec!["list", "organizations"], None)
            .await?;
        let raw: Vec<serde_json::Value> = serde_json::from_value(unwrap_data_envelope(data))
            .map_err(|e| SecretError::Backend {
                message: format!("parse orgs: {}", e),
            })?;
        Ok(raw
            .into_iter()
            .filter_map(|v| {
                Some(OrganizationRef {
                    id: v.get("id")?.as_str()?.to_string(),
                    name: v.get("name")?.as_str()?.to_string(),
                })
            })
            .collect())
    }

    async fn sync(&self) -> Result<(), SecretError> {
        // bw sync requires an unlocked session. We surface failures so callers can tell the difference between "synced ok" and "vault is locked" (which would silently leave stale data).
        self.run_with_session(vec!["sync"], None).await?;
        Ok(())
    }

    async fn init(&self) -> Result<(), SecretError> {
        self.auto_unlock_from_keychain().await;
        Ok(())
    }

    async fn collections(
        &self,
        organization_id: Option<&str>,
    ) -> Result<Vec<CollectionRef>, SecretError> {
        let mut args: Vec<&str> = vec!["list", "collections"];
        if let Some(org) = organization_id {
            args.push("--organizationid");
            args.push(org);
        }
        let data = self.run_with_session(args, None).await?;
        let raw: Vec<serde_json::Value> = serde_json::from_value(unwrap_data_envelope(data))
            .map_err(|e| SecretError::Backend {
                message: format!("parse collections: {}", e),
            })?;
        Ok(raw
            .into_iter()
            .filter_map(|v| {
                Some(CollectionRef {
                    id: v.get("id")?.as_str()?.to_string(),
                    name: v.get("name")?.as_str()?.to_string(),
                    organization_id: v
                        .get("organizationId")
                        .and_then(|x| x.as_str())
                        .map(String::from),
                })
            })
            .collect())
    }
}

use std::collections::BTreeSet;
use std::sync::Arc;

use codeterm_decision::{DecisionModel, TokenMode};
use codeterm_types::config::{SupporterConfig, SupporterConnection};
use serde::Serialize;
use serde_json::{json, Value};

use crate::SupporterRegistry;
use codeterm_mem::decision::token_store::TokenStore;

pub static TRANSACTION: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

#[derive(Debug, Serialize)]
pub struct ConnectionError {
    pub code: &'static str,
    pub message: &'static str,
}

impl ConnectionError {
    fn new(code: &'static str, message: &'static str) -> Self {
        Self { code, message }
    }
    pub fn unavailable() -> Self {
        Self::new(
            "unavailable",
            "The adapter or local storage is unavailable. Refresh and try again.",
        )
    }
}

type Result<T> = std::result::Result<T, ConnectionError>;

pub fn members(
    cfg: &SupporterConfig,
    registry: &SupporterRegistry,
    store: &TokenStore,
) -> Result<BTreeSet<String>> {
    let mut ids: BTreeSet<_> = cfg.connections.keys().cloned().collect();
    ids.extend(cfg.active_id().map(str::to_owned));
    ids.extend(
        store
            .adapters()
            .map_err(|_| ConnectionError::unavailable())?,
    );
    for id in registry.ids() {
        if store
            .status(&id)
            .map_err(|_| ConnectionError::unavailable())?
            .present
        {
            ids.insert(id);
        }
    }
    Ok(ids)
}

pub fn list(
    cfg: &SupporterConfig,
    registry: &SupporterRegistry,
    store: &TokenStore,
) -> Result<Vec<Value>> {
    let mut rows = Vec::new();
    for id in members(cfg, registry, store)? {
        let adapter = registry.get(&id);
        let token = store
            .status(&id)
            .map_err(|_| ConnectionError::unavailable())?;
        let mode = adapter.as_ref().map(|adapter| adapter.token_mode());
        let model = cfg.connection(&id).model.or_else(|| {
            adapter
                .as_ref()
                .and_then(|adapter| adapter.models().first().map(|model| model.id.clone()))
        });
        let error = if adapter.is_none() {
            Some(ConnectionError::new(
                "unavailable",
                "Install or enable this adapter, then retry activation.",
            ))
        } else if mode == Some(TokenMode::Required)
            && (!token.present
                || token.mode_ok == Some(false)
                || store.load(&id).ok().flatten().is_none())
        {
            Some(ConnectionError::new(
                "authentication_failed",
                "Replace the API token or repair its file permissions, then retry activation.",
            ))
        } else if model.as_deref().is_some_and(|model| {
            adapter
                .as_ref()
                .and_then(|adapter| adapter.model_id())
                .as_deref()
                != Some(model)
        }) {
            Some(ConnectionError::new(
                "unavailable",
                "The saved model could not be applied. Refresh the catalogue or retry activation.",
            ))
        } else {
            None
        };
        rows.push(json!({
            "adapter": id, "model": model, "active": cfg.active_id() == Some(id.as_str()),
            "token_mode": mode, "token": { "present": token.present, "mode_ok": token.mode_ok },
            "pricing": adapter.as_ref().and_then(|adapter| adapter.pricing()),
            "metadata": adapter.as_ref().map(|adapter| adapter.metadata()),
            "error": error,
        }));
    }
    rows.sort_by_key(|row| row["active"] != true);
    Ok(rows)
}

pub struct Connections<'a, F> {
    pub cfg: &'a mut SupporterConfig,
    pub registry: &'a SupporterRegistry,
    pub store: &'a TokenStore,
    pub persist: F,
}

impl<F: FnMut(&SupporterConfig) -> Result<()>> Connections<'_, F> {
    fn adapter(&self, id: &str) -> Result<Arc<dyn DecisionModel>> {
        self.registry.get(id).ok_or_else(|| {
            ConnectionError::new(
                "unknown_adapter",
                "This adapter is not registered. Install or enable it and refresh.",
            )
        })
    }

    fn model(&self, id: &str, requested: Option<String>) -> Result<Option<String>> {
        let adapter = self.adapter(id)?;
        let models = adapter.models();
        let chosen = requested
            .or_else(|| self.cfg.connection(id).model)
            .or_else(|| models.first().map(|model| model.id.clone()));
        if chosen
            .as_ref()
            .is_some_and(|id| !models.iter().any(|model| &model.id == id))
        {
            return Err(ConnectionError::new(
                "invalid_model",
                "This model is not in the adapter catalogue. Refresh and choose a listed model.",
            ));
        }
        if chosen.is_none() {
            return Err(ConnectionError::unavailable());
        }
        Ok(chosen)
    }

    fn commit(&mut self, next: SupporterConfig) -> Result<()> {
        (self.persist)(&next)?;
        *self.cfg = next;
        Ok(())
    }

    pub fn connect(
        &mut self,
        id: &str,
        requested: Option<String>,
        token: Option<&str>,
        require_new_token: bool,
    ) -> Result<Value> {
        let adapter = self.adapter(id)?;
        if members(self.cfg, self.registry, self.store)?.contains(id) {
            return Err(ConnectionError::new(
                "invalid_request",
                "This connection is already saved. Activate it or replace its token instead.",
            ));
        }
        let model = self.model(id, requested)?;
        let token = token.map(str::trim).filter(|token| !token.is_empty());
        if adapter.token_mode() == TokenMode::Required
            && token.is_none()
            && (require_new_token || !super::TokenSnapshot::read(self.store, id).usable)
        {
            return Err(ConnectionError::new(
                "authentication_failed",
                "An API token is required to connect this adapter.",
            ));
        }
        if adapter.token_mode() == TokenMode::None && token.is_some() {
            return Err(ConnectionError::new(
                "authentication_failed",
                "This adapter does not accept an API token.",
            ));
        }
        if let Some(token) = token {
            self.store
                .set(id, token)
                .map_err(|_| ConnectionError::unavailable())?;
        }
        let mut saved = self.cfg.clone();
        saved.connections.insert(
            id.into(),
            SupporterConnection {
                model: model.clone(),
                ..saved.connection(id)
            },
        );
        if let Err(error) = self.commit(saved) {
            if token.is_some() {
                return Ok(
                    json!({ "state": "saved_not_active", "adapter": id, "model": model, "error": error }),
                );
            }
            return Err(error);
        }
        match self.activate(id) {
            Ok(value) => Ok(value),
            Err(error) => Ok(
                json!({ "state": "saved_not_active", "adapter": id, "model": model, "error": error }),
            ),
        }
    }

    pub fn use_adapter(&mut self, id: &str, requested: Option<String>) -> Result<Value> {
        if !members(self.cfg, self.registry, self.store)?.contains(id) {
            return self.connect(id, requested, None, false);
        }
        let adapter = self.adapter(id)?;
        if adapter.token_mode() == TokenMode::Required
            && !super::TokenSnapshot::read(self.store, id).usable
        {
            return Err(ConnectionError::new(
                "authentication_failed",
                "Save an API token for this adapter before activating it.",
            ));
        }
        let model = self.model(id, requested)?;
        self.apply_model(id, model.clone(), true)?;
        Ok(json!({ "state": "active", "adapter": id, "model": model }))
    }

    pub fn activate(&mut self, id: &str) -> Result<Value> {
        if !members(self.cfg, self.registry, self.store)?.contains(id) {
            return Err(ConnectionError::new(
                "unknown_adapter",
                "Connect this adapter before activating it.",
            ));
        }
        let adapter = self.adapter(id)?;
        if adapter.token_mode() == TokenMode::Required
            && !super::TokenSnapshot::read(self.store, id).usable
        {
            return Err(ConnectionError::new(
                "authentication_failed",
                "Replace the API token or repair its file permissions, then retry activation.",
            ));
        }
        let model = self.model(id, None)?;
        self.apply_model(id, model.clone(), true)?;
        Ok(json!({ "state": "active", "adapter": id, "model": model }))
    }

    fn apply_model(&mut self, id: &str, model: Option<String>, activate: bool) -> Result<()> {
        let adapter = self.adapter(id)?;
        let old = adapter.model_id();
        if model.as_deref().is_some_and(|model| {
            !adapter.select_model(model) || adapter.model_id().as_deref() != Some(model)
        }) {
            if let Some(old) = old {
                adapter.select_model(&old);
            }
            return Err(ConnectionError::unavailable());
        }
        let mut next = self.cfg.clone();
        next.connections.insert(
            id.into(),
            SupporterConnection {
                model,
                ..next.connection(id)
            },
        );
        if activate {
            next.adapter = Some(id.into());
        }
        if let Err(error) = self.commit(next) {
            if let Some(old) = old {
                adapter.select_model(&old);
            }
            return Err(error);
        }
        Ok(())
    }

    pub fn set_model(&mut self, id: &str, requested: String) -> Result<Value> {
        if !members(self.cfg, self.registry, self.store)?.contains(id) {
            return Err(ConnectionError::new(
                "unknown_adapter",
                "Connect this adapter before choosing its model.",
            ));
        }
        let model = self.model(id, Some(requested))?;
        self.apply_model(id, model.clone(), false)?;
        Ok(json!({ "adapter": id, "model": model }))
    }

    pub fn replace_token(&mut self, id: &str, token: &str) -> Result<Value> {
        let adapter = self.adapter(id)?;
        if adapter.token_mode() == TokenMode::None {
            return Err(ConnectionError::new(
                "authentication_failed",
                "This adapter does not accept an API token.",
            ));
        }
        if token.trim().is_empty() {
            return Err(ConnectionError::new(
                "authentication_failed",
                "Enter a nonempty API token and retry.",
            ));
        }
        self.store
            .set(id, token)
            .map_err(|_| ConnectionError::unavailable())?;
        Ok(json!({ "adapter": id, "present": true }))
    }

    pub fn clear_token(&mut self, id: &str) -> Result<Value> {
        if !members(self.cfg, self.registry, self.store)?.contains(id) {
            self.adapter(id)?;
            return Ok(json!({ "adapter": id, "removed": false }));
        }
        let present = self
            .store
            .status(id)
            .map_err(|_| ConnectionError::unavailable())?
            .present;
        let mut next = self.cfg.clone();
        next.connections.entry(id.into()).or_default();
        let store = self.store.clone();
        store
            .remove_transaction(id, || {
                self.commit(next)
                    .map_err(|_| std::io::Error::other("configuration unavailable"))
            })
            .map_err(|_| ConnectionError::unavailable())?;
        Ok(json!({ "adapter": id, "removed": present }))
    }

    pub fn disconnect(&mut self, id: &str) -> Result<Value> {
        if !members(self.cfg, self.registry, self.store)?.contains(id) {
            return Err(ConnectionError::new(
                "unknown_adapter",
                "This connection is not saved. Refresh the connection list.",
            ));
        }
        let mut next = self.cfg.clone();
        next.connections.remove(id);
        if next.active_id() == Some(id) {
            next.adapter = None;
        }
        let store = self.store.clone();
        store
            .remove_transaction(id, || {
                self.commit(next)
                    .map_err(|_| std::io::Error::other("configuration unavailable"))
            })
            .map_err(|_| ConnectionError::unavailable())?;
        Ok(json!({ "adapter": id, "removed": true, "active": self.cfg.adapter }))
    }
}

#[cfg(test)]
mod tests;

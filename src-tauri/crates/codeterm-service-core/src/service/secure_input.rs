//! General owner-submitted secure input requests for local and Full-access mesh machines.
//!
//! Agents may create a request naming a machine, explicit consumer, kind, and reason, and may
//! poll its typed status. Only the local human Owner may submit a value, which is delivered
//! directly to the target consumer, zeroized on drop, never logged or echoed, and never
//! revealed back to agents.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use codeterm_plugin::agent_runtime::RuntimeLoginMethod;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

use crate::secrets::{SaveRequest, SecretError, SecretString as VaultSecretString};
use crate::service::agent_runtime::login::{self, LoginBinding, LoginRequest, SecretString};
use crate::service::ServiceContext;

pub const SECURE_INPUT_CHANGED_EVENT: &str = "secure-input-requests-changed";
pub const DEFAULT_TTL_SECS: u64 = 300;
pub const MAX_TTL_SECS: u64 = 900;
pub const MAX_ACTIVE_REQUESTS: usize = 64;
pub const MAX_PENDING_PER_TAB: usize = 8;
pub const MAX_RETAINED_REQUESTS: usize = 128;
pub const MAX_TARGET_DEDUPE_IDS: usize = 512;
pub const MAX_PROTECTED_SLOTS: usize = 2048;
pub const MAX_REASON_CHARS: usize = 280;
pub const MAX_VAULT_NAME_CHARS: usize = 128;
pub const MAX_VALUE_BYTES: usize = 16_384;
pub const VAULT_SAVE_TIMEOUT_SECS: u64 = 30;
pub const PROTECTED_VAULT_NOTE_MARKER: &str = "domios:secure-input:owner-only";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecureInputKind {
    Code,
    Token,
    Password,
    Key,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecureInputStatus {
    Pending,
    Submitting,
    Delivered,
    Refused,
    Expired,
    Uncertain,
}

impl SecureInputStatus {
    pub fn is_active(self) -> bool {
        matches!(self, Self::Pending | Self::Submitting)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SecureInputConsumer {
    ProviderLogin {
        tab_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        login_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        nonce: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        epoch: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        method_id: Option<String>,
    },
    VaultSlot {
        name: String,
        #[serde(default)]
        overwrite: bool,
    },
    ProviderKey {
        provider_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tab_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        method_id: Option<String>,
    },
}

#[derive(Clone, Debug, Default)]
pub struct ConsumerParseOptions {
    pub overwrite: bool,
    pub provider_id: Option<String>,
    pub login_id: Option<String>,
    pub session_id: Option<String>,
    pub nonce: Option<String>,
    pub epoch: Option<u64>,
    pub tab_id: Option<String>,
    pub method_id: Option<String>,
}

impl SecureInputConsumer {
    pub fn parse_target(raw: &str, opts: ConsumerParseOptions) -> Result<Self, String> {
        let trimmed = raw.trim();
        let consumer = if let Some(tab) = trimmed.strip_prefix("provider-login:") {
            Self::ProviderLogin {
                tab_id: tab.to_owned(),
                provider_id: clean_optional(opts.provider_id),
                login_id: clean_optional(opts.login_id),
                session_id: clean_optional(opts.session_id),
                nonce: clean_optional(opts.nonce),
                epoch: opts.epoch,
                method_id: clean_optional(opts.method_id),
            }
        } else if let Some(name) = trimmed.strip_prefix("vault:") {
            Self::VaultSlot {
                name: name.to_owned(),
                overwrite: opts.overwrite,
            }
        } else if let Some(provider) = trimmed.strip_prefix("provider-key:") {
            Self::ProviderKey {
                provider_id: provider.to_owned(),
                tab_id: clean_optional(opts.tab_id),
                method_id: clean_optional(opts.method_id),
            }
        } else {
            return Err(
                "Target must be vault:<NAME>, provider-login:<tab>, or provider-key:<provider>"
                    .to_string(),
            );
        };
        consumer.validate()?;
        Ok(consumer)
    }

    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::ProviderLogin {
                tab_id,
                provider_id,
                login_id,
                session_id,
                nonce,
                method_id,
                ..
            } => {
                validate_identifier("tab id", tab_id, 128)?;
                if let Some(provider) = provider_id.as_deref() {
                    validate_provider_id(provider)?;
                }
                if let Some(id) = login_id.as_deref() {
                    validate_identifier("login id", id, 128)?;
                }
                if let Some(id) = session_id.as_deref() {
                    validate_identifier("session id", id, 128)?;
                }
                if let Some(n) = nonce.as_deref() {
                    validate_identifier("nonce", n, 128)?;
                }
                if let Some(m) = method_id.as_deref() {
                    validate_identifier("method id", m, 64)?;
                }
                Ok(())
            }
            Self::VaultSlot { name, .. } => validate_vault_slot_name(name),
            Self::ProviderKey {
                provider_id,
                tab_id,
                method_id,
            } => {
                validate_provider_id(provider_id)?;
                if let Some(tab) = tab_id.as_deref() {
                    validate_identifier("tab id", tab, 128)?;
                }
                if let Some(m) = method_id.as_deref() {
                    validate_identifier("method id", m, 64)?;
                }
                Ok(())
            }
        }
    }

    pub fn validate_remote_requirements(&self) -> Result<(), String> {
        if let Self::ProviderLogin {
            provider_id,
            login_id,
            nonce,
            epoch,
            ..
        } = self
        {
            if provider_id.is_none() || login_id.is_none() || nonce.is_none() || epoch.is_none() {
                return Err(
                    "Remote provider-login requires provider_id, login_id, nonce, and epoch"
                        .to_owned(),
                );
            }
        }
        Ok(())
    }

    pub fn target_label(&self) -> String {
        match self {
            Self::ProviderLogin { tab_id, .. } => format!("provider-login:{tab_id}"),
            Self::VaultSlot { name, .. } => format!("vault:{name}"),
            Self::ProviderKey { provider_id, .. } => format!("provider-key:{provider_id}"),
        }
    }
}

fn clean_optional(value: Option<String>) -> Option<String> {
    value.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty())
}

pub fn is_reserved_vault_slot(name: &str) -> bool {
    let lower = name.trim().to_ascii_lowercase();
    lower.starts_with("domios.instance:")
        || lower.starts_with("domios:instance:")
        || lower.starts_with("codeterm.plugin.")
}

pub fn is_always_owner_only_vault_slot(name: &str) -> bool {
    let lower = name.trim().to_ascii_lowercase();
    is_reserved_vault_slot(&lower)
        || lower.starts_with("domios:provider:")
        || lower.starts_with("domios:git:")
}

pub fn validate_vault_slot_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name != name.trim() {
        return Err("Vault slot name must be non-empty without leading or trailing spaces".into());
    }
    if name == "." || name == ".." {
        return Err("Vault slot name must not be '.' or '..'".into());
    }
    if name.chars().count() > MAX_VAULT_NAME_CHARS {
        return Err(format!(
            "Vault slot name must be at most {MAX_VAULT_NAME_CHARS} characters"
        ));
    }
    if name.chars().any(|c| c.is_control() || c == '\0') {
        return Err("Vault slot name must not contain control characters".into());
    }
    if is_reserved_vault_slot(name) {
        return Err(format!(
            "Vault slot '{name}' is reserved for host authority"
        ));
    }
    Ok(())
}

pub fn validate_provider_id(provider: &str) -> Result<(), String> {
    if provider.is_empty() || provider.len() > 64 {
        return Err("Provider id must be 1..=64 characters".into());
    }
    if provider == "." || provider == ".." {
        return Err("Provider id must not be '.' or '..'".into());
    }
    if !provider
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    {
        return Err("Provider id contains invalid characters".into());
    }
    Ok(())
}

fn validate_identifier(label: &str, value: &str, max_len: usize) -> Result<(), String> {
    if value.is_empty() || value.len() > max_len {
        return Err(format!("{label} must be 1..={max_len} characters"));
    }
    if value == "." || value == ".." {
        return Err(format!("{label} must not be '.' or '..'"));
    }
    if !value
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':'))
    {
        return Err(format!("{label} contains invalid characters"));
    }
    Ok(())
}

pub fn validate_reason(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Secure input request requires a non-empty reason shown to the owner".into());
    }
    if trimmed.chars().count() > MAX_REASON_CHARS {
        return Err(format!(
            "Secure input reason must be at most {MAX_REASON_CHARS} characters"
        ));
    }
    if trimmed.chars().any(|c| c.is_control()) {
        return Err("Secure input reason must not contain control characters".into());
    }
    Ok(trimmed.to_owned())
}

/// Metadata-only view of a secure input request. Never contains any secret bytes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecureInputRequestView {
    pub request_id: String,
    pub target_installation_id: String,
    pub target_machine_name: String,
    pub is_remote: bool,
    pub consumer: SecureInputConsumer,
    pub target_label: String,
    pub kind: SecureInputKind,
    pub reason: String,
    pub status: SecureInputStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason_detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requester_tab_id: Option<String>,
    pub created_at_ms: u64,
    pub expires_at_ms: u64,
}

#[derive(Clone, Debug)]
pub struct SecureInputCreateParams {
    pub target_installation_id: String,
    pub target_machine_name: String,
    pub is_remote: bool,
    pub consumer: SecureInputConsumer,
    pub kind: SecureInputKind,
    pub reason: String,
    pub requester_tab_id: Option<String>,
    pub ttl_secs: Option<u64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SecureInputDeliveryPayload {
    pub request_id: String,
    pub target_installation_id: String,
    pub consumer: SecureInputConsumer,
    pub kind: SecureInputKind,
    pub expires_at_ms: u64,
    pub value: SecretString,
}

impl std::fmt::Debug for SecureInputDeliveryPayload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecureInputDeliveryPayload")
            .field("request_id", &self.request_id)
            .field("target_installation_id", &self.target_installation_id)
            .field("consumer", &self.consumer)
            .field("kind", &self.kind)
            .field("expires_at_ms", &self.expires_at_ms)
            .field("value", &"<redacted>")
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecureInputDeliveryResult {
    pub status: SecureInputStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SecureInputError {
    BadRequest(String),
    NotFound(String),
    Forbidden(String),
    Conflict(String),
}

impl SecureInputError {
    pub fn message(&self) -> &str {
        match self {
            Self::BadRequest(m) | Self::NotFound(m) | Self::Forbidden(m) | Self::Conflict(m) => m,
        }
    }
}

/// Zeroizes any string values stored under sensitive keys in a JSON tree.
pub fn zeroize_secure_input_json_value(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (k, v) in map.iter_mut() {
                if matches!(
                    k.as_str(),
                    "value" | "code" | "apiKey" | "api_key" | "secret" | "token" | "password"
                ) {
                    if let serde_json::Value::String(secret) = v {
                        secret.zeroize();
                    }
                } else {
                    zeroize_secure_input_json_value(v);
                }
            }
        }
        serde_json::Value::Array(items) => {
            for item in items.iter_mut() {
                zeroize_secure_input_json_value(item);
            }
        }
        _ => {}
    }
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

struct StoredRequest {
    view: SecureInputRequestView,
    in_flight: bool,
}

#[derive(Default)]
struct RegistryInner {
    requests: HashMap<String, StoredRequest>,
    order: VecDeque<String>,
    delivered_target_ids: VecDeque<String>,
    delivered_target_set: HashSet<String>,
    protected_vault_slots: HashSet<String>,
    protected_slots_path: Option<PathBuf>,
}

pub struct SecureInputRegistry {
    inner: Mutex<RegistryInner>,
}

impl Default for SecureInputRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl SecureInputRegistry {
    pub fn new() -> Self {
        Self::new_at(None)
    }

    pub fn new_at(protected_slots_path: Option<PathBuf>) -> Self {
        let protected_vault_slots = protected_slots_path
            .as_deref()
            .map(load_protected_slots)
            .unwrap_or_default();
        Self {
            inner: Mutex::new(RegistryInner {
                protected_vault_slots,
                protected_slots_path,
                ..RegistryInner::default()
            }),
        }
    }

    pub fn create(
        &self,
        params: SecureInputCreateParams,
        now: u64,
    ) -> Result<SecureInputRequestView, SecureInputError> {
        if params.target_installation_id.trim().is_empty() {
            return Err(SecureInputError::BadRequest(
                "Target machine installation id is required".into(),
            ));
        }
        params
            .consumer
            .validate()
            .map_err(SecureInputError::BadRequest)?;
        if params.is_remote {
            params
                .consumer
                .validate_remote_requirements()
                .map_err(SecureInputError::BadRequest)?;
        }
        let reason = validate_reason(&params.reason).map_err(SecureInputError::BadRequest)?;
        let ttl_secs = params
            .ttl_secs
            .unwrap_or(DEFAULT_TTL_SECS)
            .clamp(30, MAX_TTL_SECS);
        let request_id = format!("sir_{}", uuid::Uuid::new_v4().simple());
        let target_label = params.consumer.target_label();
        let view = SecureInputRequestView {
            request_id: request_id.clone(),
            target_installation_id: params.target_installation_id,
            target_machine_name: params.target_machine_name,
            is_remote: params.is_remote,
            consumer: params.consumer,
            target_label,
            kind: params.kind,
            reason,
            status: SecureInputStatus::Pending,
            reason_detail: None,
            requester_tab_id: params.requester_tab_id.clone(),
            created_at_ms: now,
            expires_at_ms: now.saturating_add(ttl_secs.saturating_mul(1000)),
        };

        let mut inner = self.inner.lock();
        expire_stale_locked(&mut inner, now);
        let active_count = inner
            .requests
            .values()
            .filter(|entry| entry.in_flight || entry.view.status.is_active())
            .count();
        if active_count >= MAX_ACTIVE_REQUESTS {
            return Err(SecureInputError::Conflict(
                "Too many active secure input requests; resolve or cancel older requests first"
                    .into(),
            ));
        }
        if let Some(tab_id) = params.requester_tab_id.as_deref() {
            let tab_active = inner
                .requests
                .values()
                .filter(|entry| {
                    (entry.in_flight || entry.view.status.is_active())
                        && entry.view.requester_tab_id.as_deref() == Some(tab_id)
                })
                .count();
            if tab_active >= MAX_PENDING_PER_TAB {
                return Err(SecureInputError::Conflict(
                    "Too many active secure input requests for this tab".into(),
                ));
            }
        }
        // Evict only settled entries so an agent cannot displace Pending or in-flight owner cards.
        while inner.order.len() >= MAX_RETAINED_REQUESTS {
            let settled_idx = inner.order.iter().position(|id| {
                inner
                    .requests
                    .get(id)
                    .is_none_or(|entry| !entry.in_flight && !entry.view.status.is_active())
            });
            let Some(idx) = settled_idx else {
                return Err(SecureInputError::Conflict(
                    "Secure input request queue is full of active requests".into(),
                ));
            };
            if let Some(evicted) = inner.order.remove(idx) {
                inner.requests.remove(&evicted);
            }
        }
        inner.order.push_back(request_id.clone());
        inner.requests.insert(
            request_id,
            StoredRequest {
                view: view.clone(),
                in_flight: false,
            },
        );
        Ok(view)
    }

    pub fn list(&self, now: u64) -> Vec<SecureInputRequestView> {
        let mut inner = self.inner.lock();
        expire_stale_locked(&mut inner, now);
        inner
            .order
            .iter()
            .rev()
            .filter_map(|id| inner.requests.get(id).map(|entry| entry.view.clone()))
            .collect()
    }

    pub fn get(&self, request_id: &str, now: u64) -> Option<SecureInputRequestView> {
        let mut inner = self.inner.lock();
        expire_stale_locked(&mut inner, now);
        inner
            .requests
            .get(request_id)
            .map(|entry| entry.view.clone())
    }

    pub fn cancel(
        &self,
        request_id: &str,
        requester_tab: Option<&str>,
        is_owner: bool,
        detail: Option<&str>,
        now: u64,
    ) -> Result<SecureInputRequestView, SecureInputError> {
        let mut inner = self.inner.lock();
        expire_stale_locked(&mut inner, now);
        let entry = inner.requests.get_mut(request_id).ok_or_else(|| {
            SecureInputError::NotFound(format!("Secure input request '{request_id}' not found"))
        })?;
        if !is_owner && entry.view.requester_tab_id.as_deref() != requester_tab {
            return Err(SecureInputError::Forbidden(
                "Only the owner or the requesting tab may cancel this secure input request".into(),
            ));
        }
        if entry.in_flight || entry.view.status == SecureInputStatus::Submitting {
            return Err(SecureInputError::Conflict(
                "Secure input submission is already in flight and cannot be cancelled".into(),
            ));
        }
        if entry.view.status != SecureInputStatus::Pending {
            return Err(SecureInputError::Conflict(format!(
                "Secure input request is already {:?}",
                entry.view.status
            )));
        }
        entry.view.status = SecureInputStatus::Refused;
        entry.view.reason_detail = Some(
            detail
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or("Refused")
                .to_owned(),
        );
        Ok(entry.view.clone())
    }

    /// Atomically claims a pending request for single-use owner submission, transitioning it
    /// to `Submitting` (`in_flight = true`). Rejects expired, settled, uncertain, in-flight,
    /// or mismatched-target requests.
    pub fn claim_submission(
        &self,
        request_id: &str,
        expected_installation_id: Option<&str>,
        now: u64,
    ) -> Result<SecureInputRequestView, SecureInputError> {
        let mut inner = self.inner.lock();
        expire_stale_locked(&mut inner, now);
        let entry = inner.requests.get_mut(request_id).ok_or_else(|| {
            SecureInputError::NotFound(format!("Secure input request '{request_id}' not found"))
        })?;
        if let Some(expected) = expected_installation_id {
            if entry.view.target_installation_id != expected {
                return Err(SecureInputError::Conflict(
                    "Secure input request target machine mismatch".into(),
                ));
            }
        }
        if entry.in_flight || entry.view.status == SecureInputStatus::Submitting {
            return Err(SecureInputError::Conflict(
                "Secure input request is already being submitted".into(),
            ));
        }
        match entry.view.status {
            SecureInputStatus::Pending => {
                entry.in_flight = true;
                entry.view.status = SecureInputStatus::Submitting;
                Ok(entry.view.clone())
            }
            SecureInputStatus::Submitting => Err(SecureInputError::Conflict(
                "Secure input request is already being submitted".into(),
            )),
            SecureInputStatus::Delivered => Err(SecureInputError::Conflict(
                "Secure input request was already delivered and cannot be replayed".into(),
            )),
            SecureInputStatus::Uncertain => Err(SecureInputError::Conflict(
                "Previous submission outcome was uncertain; create a new request instead of replaying"
                    .into(),
            )),
            SecureInputStatus::Expired => Err(SecureInputError::Conflict(
                "Secure input request has expired".into(),
            )),
            SecureInputStatus::Refused => Err(SecureInputError::Conflict(
                "Secure input request was already refused".into(),
            )),
        }
    }

    pub fn settle_submission(
        &self,
        request_id: &str,
        status: SecureInputStatus,
        detail: Option<String>,
    ) -> Option<SecureInputRequestView> {
        let mut inner = self.inner.lock();
        let entry = inner.requests.get_mut(request_id)?;
        entry.in_flight = false;
        entry.view.status = status;
        entry.view.reason_detail = detail
            .map(|s| sanitize_detail(&s))
            .filter(|s| !s.is_empty());
        Some(entry.view.clone())
    }

    /// Target-side single-use replay guard: returns `true` only on the first delivery attempt
    /// for a given `request_id`.
    pub fn claim_target_delivery(&self, request_id: &str) -> bool {
        let mut inner = self.inner.lock();
        if inner.delivered_target_set.contains(request_id) {
            return false;
        }
        while inner.delivered_target_ids.len() >= MAX_TARGET_DEDUPE_IDS {
            if let Some(oldest) = inner.delivered_target_ids.pop_front() {
                inner.delivered_target_set.remove(&oldest);
            }
        }
        inner.delivered_target_ids.push_back(request_id.to_owned());
        inner.delivered_target_set.insert(request_id.to_owned());
        true
    }

    pub fn mark_protected_vault_slot(&self, name: &str) {
        let key = name.trim().to_ascii_lowercase();
        if key.is_empty() {
            return;
        }
        let persist_target = {
            let mut inner = self.inner.lock();
            if inner.protected_vault_slots.len() >= MAX_PROTECTED_SLOTS
                && !inner.protected_vault_slots.contains(&key)
            {
                return;
            }
            if !inner.protected_vault_slots.insert(key) {
                return;
            }
            inner.protected_slots_path.as_ref().map(|path| {
                let mut slots: Vec<String> = inner.protected_vault_slots.iter().cloned().collect();
                slots.sort();
                (path.clone(), slots)
            })
        };
        if let Some((path, slots)) = persist_target {
            persist_protected_slots(&path, &slots);
        }
    }

    pub fn is_protected_vault_slot(&self, name: &str) -> bool {
        if is_always_owner_only_vault_slot(name) {
            return true;
        }
        let key = name.trim().to_ascii_lowercase();
        self.inner.lock().protected_vault_slots.contains(&key)
    }
}

fn load_protected_slots(path: &Path) -> HashSet<String> {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return HashSet::new();
    };
    let Ok(list) = serde_json::from_str::<Vec<String>>(&raw) else {
        return HashSet::new();
    };
    list.into_iter()
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .take(MAX_PROTECTED_SLOTS)
        .collect()
}

fn persist_protected_slots(path: &Path, slots: &[String]) {
    let Some(parent) = path.parent() else {
        return;
    };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }
    let Ok(json) = serde_json::to_vec_pretty(slots) else {
        return;
    };
    let tmp = path.with_extension(format!("tmp-{}", uuid::Uuid::new_v4().simple()));
    if std::fs::write(&tmp, &json).is_err() {
        return;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
    }
    if std::fs::rename(&tmp, path).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

fn expire_stale_locked(inner: &mut RegistryInner, now: u64) {
    for entry in inner.requests.values_mut() {
        if entry.view.status == SecureInputStatus::Pending
            && !entry.in_flight
            && entry.view.expires_at_ms <= now
        {
            entry.view.status = SecureInputStatus::Expired;
            entry.view.reason_detail = Some("Request expired before submission".into());
        }
    }
}

pub fn sanitize_detail(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() > 240 {
        collapsed.chars().take(239).chain(['…']).collect()
    } else {
        collapsed
    }
}

pub fn scrub_secret(message: &str, secret: &str) -> String {
    let mut scrubbed = if secret.is_empty() {
        message.to_owned()
    } else {
        message.replace(secret, "<redacted>")
    };
    let trimmed = secret.trim();
    if !trimmed.is_empty() && trimmed != secret {
        scrubbed = scrubbed.replace(trimmed, "<redacted>");
    }
    sanitize_detail(&scrubbed)
}

/// Emits a metadata-free invalidation signal so the owner UI refreshes pending requests via the
/// owner-authenticated `GET /api/secure-input/requests` route. Never carries request IDs, targets,
/// reasons, or login bindings on the shared event bus.
pub fn emit_requests_changed(svc: &ServiceContext) {
    svc.ui
        .emit(SECURE_INPUT_CHANGED_EVENT, serde_json::json!({}));
}

/// Target-machine consumer delivery. Validates binding, expiry, and single-use dedupe,
/// routes the value strictly to the declared consumer, and never logs or echoes the secret.
pub async fn deliver_to_consumer(
    svc: &ServiceContext,
    own_installation_id: &str,
    is_remote_delivery: bool,
    payload: SecureInputDeliveryPayload,
    now: u64,
) -> SecureInputDeliveryResult {
    if payload.request_id.trim().is_empty() {
        return SecureInputDeliveryResult {
            status: SecureInputStatus::Refused,
            reason: Some("Missing secure input request id".into()),
        };
    }
    if payload.target_installation_id != own_installation_id {
        return SecureInputDeliveryResult {
            status: SecureInputStatus::Refused,
            reason: Some("Secure input payload belongs to a different installation".into()),
        };
    }
    if payload.expires_at_ms <= now {
        return SecureInputDeliveryResult {
            status: SecureInputStatus::Expired,
            reason: Some("Secure input request has expired".into()),
        };
    }
    if let Err(reason) = payload.consumer.validate() {
        return SecureInputDeliveryResult {
            status: SecureInputStatus::Refused,
            reason: Some(reason),
        };
    }
    if is_remote_delivery {
        if let Err(reason) = payload.consumer.validate_remote_requirements() {
            return SecureInputDeliveryResult {
                status: SecureInputStatus::Refused,
                reason: Some(reason),
            };
        }
    }
    let raw_len = payload.value.as_str().len();
    if payload.value.as_str().trim().is_empty() || raw_len > MAX_VALUE_BYTES {
        return SecureInputDeliveryResult {
            status: SecureInputStatus::Refused,
            reason: Some("Secret value must be non-empty and within size limits".into()),
        };
    }
    if payload.value.as_str().contains('\0') {
        return SecureInputDeliveryResult {
            status: SecureInputStatus::Refused,
            reason: Some("Secret value must not contain NUL bytes".into()),
        };
    }
    if !svc
        .secure_input()
        .claim_target_delivery(&payload.request_id)
    {
        return SecureInputDeliveryResult {
            status: SecureInputStatus::Refused,
            reason: Some("Secure input request was already consumed".into()),
        };
    }

    match payload.consumer {
        SecureInputConsumer::VaultSlot { name, overwrite } => {
            deliver_vault_slot(svc, &name, overwrite, payload.value).await
        }
        SecureInputConsumer::ProviderLogin {
            tab_id,
            provider_id,
            login_id,
            session_id,
            nonce,
            epoch,
            method_id,
        } => {
            deliver_provider_login(
                svc,
                own_installation_id,
                &tab_id,
                provider_id,
                login_id,
                session_id,
                nonce,
                epoch,
                method_id,
                payload.value,
            )
            .await
        }
        SecureInputConsumer::ProviderKey {
            provider_id,
            tab_id,
            method_id,
        } => deliver_provider_key(svc, &provider_id, tab_id, method_id, payload.value).await,
    }
}

async fn deliver_vault_slot(
    svc: &ServiceContext,
    name: &str,
    overwrite: bool,
    value: SecretString,
) -> SecureInputDeliveryResult {
    // Persist protection in host-owned state before saving so even an uncertain write
    // can never be revealed, overwritten, or deleted by agents.
    svc.secure_input().mark_protected_vault_slot(name);
    let store = svc.secrets.read().await.clone();
    let save_req = SaveRequest {
        name: name.to_owned(),
        value: VaultSecretString::from(value.as_str().to_owned()),
        notes: None,
        folder: None,
        organization: None,
        collection: None,
        overwrite,
    };
    let save_outcome = tokio::time::timeout(
        Duration::from_secs(VAULT_SAVE_TIMEOUT_SECS),
        store.save(save_req),
    )
    .await;
    match save_outcome {
        Err(_) => SecureInputDeliveryResult {
            status: SecureInputStatus::Uncertain,
            reason: Some(
                "Vault write timed out; outcome is uncertain — verify the slot before retrying"
                    .into(),
            ),
        },
        Ok(Ok(_)) => SecureInputDeliveryResult {
            status: SecureInputStatus::Delivered,
            reason: None,
        },
        Ok(Err(SecretError::WriteUncertain { .. })) => SecureInputDeliveryResult {
            status: SecureInputStatus::Uncertain,
            reason: Some(
                "Vault write outcome was uncertain; verify the slot before retrying".into(),
            ),
        },
        Ok(Err(SecretError::AlreadyExists)) => SecureInputDeliveryResult {
            status: SecureInputStatus::Refused,
            reason: Some(format!(
                "Vault slot '{name}' already exists; request with --overwrite to replace it"
            )),
        },
        Ok(Err(SecretError::Locked)) => SecureInputDeliveryResult {
            status: SecureInputStatus::Refused,
            reason: Some("Target secret vault is locked".into()),
        },
        Ok(Err(SecretError::LoggedOut)) => SecureInputDeliveryResult {
            status: SecureInputStatus::Refused,
            reason: Some("Target secret vault is logged out".into()),
        },
        Ok(Err(other)) => SecureInputDeliveryResult {
            status: SecureInputStatus::Refused,
            reason: Some(format!("Vault save failed ({})", other.redacted().kind())),
        },
    }
}

#[allow(clippy::too_many_arguments)]
async fn deliver_provider_login(
    svc: &ServiceContext,
    own_installation_id: &str,
    tab_id: &str,
    provider_id: Option<String>,
    login_id: Option<String>,
    session_id: Option<String>,
    nonce: Option<String>,
    epoch: Option<u64>,
    method_id: Option<String>,
    value: SecretString,
) -> SecureInputDeliveryResult {
    let secret_copy = value.clone().into_zeroizing();
    let binding = match (nonce, epoch) {
        (Some(nonce), Some(epoch)) => Some(LoginBinding {
            installation_id: Some(own_installation_id.to_owned()),
            tab_id: Some(tab_id.to_owned()),
            provider_id: provider_id.clone(),
            session_id,
            nonce,
            epoch,
        }),
        _ => None,
    };
    let request = LoginRequest {
        binding,
        login_id,
        action: Some("code".to_owned()),
        owner_confirmed: true,
        provider_id,
        account: None,
        code: Some(value),
        method_id,
        method: None,
        api_key: None,
    };
    match login::operate(svc, tab_id, request).await {
        Ok(val) => {
            let status = val
                .get("codeSubmission")
                .and_then(|c| c.get("status"))
                .and_then(|s| s.as_str());
            let detail = val
                .get("codeSubmission")
                .and_then(|c| c.get("reason"))
                .and_then(|s| s.as_str())
                .map(|r| scrub_secret(r, secret_copy.as_str()));
            match status {
                Some("uncertain") => SecureInputDeliveryResult {
                    status: SecureInputStatus::Uncertain,
                    reason: detail.or_else(|| {
                        Some("Code delivery to provider was uncertain; do not replay".into())
                    }),
                },
                Some("failed") => SecureInputDeliveryResult {
                    status: SecureInputStatus::Refused,
                    reason: detail.or_else(|| Some("Provider refused sign-in code".into())),
                },
                _ => SecureInputDeliveryResult {
                    status: SecureInputStatus::Delivered,
                    reason: None,
                },
            }
        }
        Err(reason) => {
            let safe = scrub_secret(&reason, secret_copy.as_str());
            let lower = safe.to_ascii_lowercase();
            let status = if lower.contains("uncertain") || lower.contains("unconfirmed") {
                SecureInputStatus::Uncertain
            } else {
                SecureInputStatus::Refused
            };
            SecureInputDeliveryResult {
                status,
                reason: Some(safe),
            }
        }
    }
}

async fn deliver_provider_key(
    svc: &ServiceContext,
    provider_id: &str,
    tab_id: Option<String>,
    method_id: Option<String>,
    value: SecretString,
) -> SecureInputDeliveryResult {
    let slot_name = format!("domios:provider:{provider_id}:api_key");
    svc.secure_input().mark_protected_vault_slot(&slot_name);
    if let Some(tab) = tab_id {
        let secret_copy = value.clone().into_zeroizing();
        let request = LoginRequest {
            binding: None,
            login_id: None,
            action: if method_id.is_some() {
                None
            } else {
                Some("start".to_owned())
            },
            owner_confirmed: true,
            provider_id: Some(provider_id.to_owned()),
            account: None,
            code: None,
            method_id: method_id.clone(),
            method: if method_id.is_some() {
                None
            } else {
                Some(RuntimeLoginMethod::ApiKey)
            },
            api_key: Some(value),
        };
        return match login::operate(svc, &tab, request).await {
            Ok(_) => SecureInputDeliveryResult {
                status: SecureInputStatus::Delivered,
                reason: None,
            },
            Err(reason) => {
                let safe = scrub_secret(&reason, secret_copy.as_str());
                let lower = safe.to_ascii_lowercase();
                let status = if lower.contains("uncertain")
                    || lower.contains("unconfirmed")
                    || lower.contains("timed out")
                    || lower.contains("was saved")
                {
                    SecureInputStatus::Uncertain
                } else {
                    SecureInputStatus::Refused
                };
                SecureInputDeliveryResult {
                    status,
                    reason: Some(safe),
                }
            }
        };
    }
    deliver_vault_slot(svc, &slot_name, true, value).await
}

impl ServiceContext {
    pub fn secure_input(&self) -> &Arc<SecureInputRegistry> {
        self.ext_or_init(|| {
            let path = self
                .agent_pad_data_dir
                .join("secrets")
                .join("protected-vault-slots.json");
            Arc::new(SecureInputRegistry::new_at(Some(path)))
        })
    }
}

#[cfg(test)]
mod tests;

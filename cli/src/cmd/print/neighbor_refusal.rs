use super::CliError;
use crate::cmd::error_contract::is_neighbor_reason;

impl CliError {
    pub(crate) fn neighbor_refusal(&self) -> Option<Self> {
        if self.code != Some(-32003) && self.data.as_ref().and_then(|data| data.get("http_status")).and_then(serde_json::Value::as_u64) != Some(403) { return None; }
        let reason = self.message.split_once(':').map(|(reason, _)| reason)
            .filter(|reason| is_neighbor_reason(reason))
            .or_else(|| self.data.as_ref()?.get("reason")?.as_str().filter(|reason| is_neighbor_reason(reason)))?;
        let safe = self.message.chars().count() <= 1024
            && !self.message.chars().any(|character| character.is_control() || matches!(character, '{' | '}' | '[' | ']' | '"' | '\\'));
        let message = if safe {
            if self.message.starts_with(&format!("{reason}:")) { self.message.clone() }
            else { format!("{reason}: {}", self.message) }
        } else {
            format!("{reason}: This Neighbor operation is not allowed")
        };
        Some(Self {
            message, code: self.code,
            data: Some(serde_json::json!({"code":"refused","reason":reason,"http_status":403})),
        })
    }
}

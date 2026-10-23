use crate::blueprint_document::{Limits, Port};
use crate::blueprint_journal::JournalReason;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub const MAX_STDIN_BYTES: usize = 32 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionEnvelope {
    pub media: String,
    #[serde(deserialize_with = "schema_id")]
    pub schema_id: Option<String>,
    pub value: Value,
    pub text: String,
}

fn schema_id<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    Option::<String>::deserialize(deserializer)
}

impl ExecutionEnvelope {
    pub fn from_text(text: &str) -> Self {
        match serde_json::from_str::<Value>(text) {
            Ok(value) => Self {
                media: "json".into(),
                schema_id: None,
                value,
                text: String::new(),
            },
            Err(_) => Self {
                media: "text".into(),
                schema_id: None,
                value: Value::String(text.into()),
                text: String::new(),
            },
        }
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, JournalReason> {
        if bytes.len() > MAX_STDIN_BYTES {
            return Err(JournalReason::ContractViolation);
        }
        let envelope: Self =
            serde_json::from_slice(bytes).map_err(|_| JournalReason::ContractViolation)?;
        envelope.validate_shape()?;
        Ok(envelope)
    }

    pub fn encode(&self) -> Result<Vec<u8>, JournalReason> {
        self.validate_shape()?;
        let bytes = serde_json::to_vec(self).map_err(|_| JournalReason::ContractViolation)?;
        if bytes.len() > MAX_STDIN_BYTES {
            let payload = self.payload_text();
            return serde_json::to_vec(&Self {
                media: "json".into(), schema_id: None, text: String::new(),
                value: serde_json::json!({"type":"truncated", "truncated":true,
                    "total_bytes":payload.len(), "limit_bytes":MAX_STDIN_BYTES,
                    "prefix":payload.chars().take((MAX_STDIN_BYTES - 512) / 6).collect::<String>()}),
            }).map_err(|_| JournalReason::ContractViolation);
        }
        Ok(bytes)
    }

    fn validate_shape(&self) -> Result<(), JournalReason> {
        if !matches!(
            self.media.as_str(),
            "json" | "text" | "file" | "audio" | "video" | "image"
        ) || (self.media == "text" && (!self.value.is_string() || !self.text.is_empty()))
            || (self.media == "json" && !self.text.is_empty())
            || self.schema_id.as_ref().is_some_and(|id| id.len() > 256)
        {
            return Err(JournalReason::ContractViolation);
        }
        Ok(())
    }

    pub fn payload_text(&self) -> String {
        if self.media == "text" {
            self.value.as_str().unwrap_or_default().into()
        } else {
            self.value.to_string()
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionContract {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_binding: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<Port>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<Port>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub types: BTreeMap<String, Port>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limits: Option<Limits>,
}

impl ExecutionContract {
    pub fn validate(
        &self,
        envelope: &ExecutionEnvelope,
        output: bool,
    ) -> Result<(), JournalReason> {
        let bounded = ExecutionEnvelope::decode(&envelope.encode()?)?;
        let envelope = &bounded;
        let port = if output { &self.output } else { &self.input };
        if let Some(port) = port {
            if !port.accepts_with_types(&envelope.value, &self.types)
                || port
                    .media
                    .as_ref()
                    .is_some_and(|media| media != &envelope.media)
                || port
                    .reference
                    .as_ref()
                    .is_some_and(|id| envelope.schema_id.as_ref() != Some(id))
            {
                return Err(JournalReason::ContractViolation);
            }
        }
        Ok(())
    }
}

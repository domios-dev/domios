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

    /// Trigger text is a string; it is read as JSON only when the first node's input schema rejects the string.
    pub fn from_trigger_text(text: &str, first: Option<&ExecutionContract>) -> Self {
        let literal = Self {
            media: "text".into(),
            schema_id: None,
            value: Value::String(text.into()),
            text: String::new(),
        };
        let Some((port, types)) = first.and_then(|c| c.input.as_ref().map(|p| (p, &c.types)))
        else {
            return literal;
        };
        if port.accepts_with_types(&literal.value, types) {
            return literal;
        }
        match serde_json::from_str::<Value>(text) {
            Ok(value) if port.accepts_with_types(&value, types) => Self {
                media: "json".into(),
                schema_id: None,
                value,
                text: String::new(),
            },
            _ => literal,
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
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub input_bindings: BTreeMap<String, String>,
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
    pub fn bind_input(
        &self,
        input: &ExecutionEnvelope,
        mut resolve: impl FnMut(&str) -> Result<ExecutionEnvelope, JournalReason>,
    ) -> Result<ExecutionEnvelope, JournalReason> {
        if let Some(binding) = &self.input_binding {
            if !self.input_bindings.is_empty() {
                return Err(JournalReason::ContractViolation);
            }
            return resolve(binding);
        }
        if self.input_bindings.is_empty() {
            return Ok(input.clone());
        }
        let value = self
            .input_bindings
            .iter()
            .map(|(name, binding)| resolve(binding).map(|envelope| (name.clone(), envelope.value)))
            .collect::<Result<serde_json::Map<_, _>, _>>()?;
        Ok(ExecutionEnvelope {
            media: "json".into(),
            schema_id: self.input.as_ref().and_then(|p| p.reference.clone()),
            value: Value::Object(value),
            text: String::new(),
        })
    }

    pub fn validate(
        &self,
        envelope: &ExecutionEnvelope,
        output: bool,
    ) -> Result<(), JournalReason> {
        self.violation(envelope, output)?
            .map_or(Ok(()), |_| Err(JournalReason::ContractViolation))
    }

    /// `Err` means the envelope is malformed or oversize; `Ok(Some(_))` names the mismatched port and value.
    pub fn violation(
        &self,
        envelope: &ExecutionEnvelope,
        output: bool,
    ) -> Result<Option<String>, JournalReason> {
        let bounded = ExecutionEnvelope::decode(&envelope.encode()?)?;
        let envelope = &bounded;
        let side = if output { "output" } else { "input" };
        let port = if output { &self.output } else { &self.input };
        let Some(port) = port else {
            return Ok(None);
        };
        let mismatched = !port.accepts_with_types(&envelope.value, &self.types)
            || port.media.as_ref().is_some_and(|m| m != &envelope.media)
            || port
                .reference
                .as_ref()
                .is_some_and(|id| envelope.schema_id.as_ref() != Some(id));
        if !mismatched {
            return Ok(None);
        }
        let schema = serde_json::to_string(port).unwrap_or_default();
        Ok(Some(format!(
            "{side} expected {} got {} {}",
            clip(&schema, 80),
            envelope.media,
            clip(&envelope.value.to_string(), 50),
        )))
    }
}

fn clip(text: &str, limit: usize) -> String {
    let mut clipped: String = text.chars().take(limit).collect();
    if text.chars().count() > limit {
        clipped.push('…');
    }
    clipped
}

#[cfg(test)]
mod tests;

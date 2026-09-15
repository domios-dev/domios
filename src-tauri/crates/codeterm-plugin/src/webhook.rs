//! Host-facing webhook receiver capability.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::capability::CapabilityRegistry;
use crate::js_runtime::manifest::WebhookReceiverKind;
use crate::js_runtime::VmError;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WebhookReceiveContext {
    pub payload: Value,
    pub received_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookDelivery {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,
}

impl<'de> Deserialize<'de> for WebhookDelivery {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let object = value
            .as_object()
            .ok_or_else(|| serde::de::Error::custom("delivery must be an object"))?;
        for field in object.keys() {
            if field != "text" && field != "eventId" {
                return Err(serde::de::Error::custom(format!(
                    "unknown delivery field `{field}`"
                )));
            }
        }
        let text = object
            .get("text")
            .and_then(Value::as_str)
            .ok_or_else(|| serde::de::Error::custom("delivery text must be a string"))?;
        let event_id = match object.get("eventId") {
            None => None,
            Some(Value::String(event_id)) => Some(event_id.clone()),
            Some(_) => {
                return Err(serde::de::Error::custom(
                    "eventId must be omitted or a string",
                ))
            }
        };
        Ok(Self {
            text: text.to_string(),
            event_id,
        })
    }
}

#[derive(Debug)]
pub enum WebhookReceiveError {
    ContextSerialization(serde_json::Error),
    Vm(VmError),
}

impl std::fmt::Display for WebhookReceiveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ContextSerialization(error) => {
                write!(f, "webhook context serialization failed: {error}")
            }
            Self::Vm(error) => write!(f, "webhook receiver failed: {error}"),
        }
    }
}

impl std::error::Error for WebhookReceiveError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ContextSerialization(error) => Some(error),
            Self::Vm(error) => Some(error),
        }
    }
}

pub trait WebhookReceiver: Send + Sync {
    fn id(&self) -> &str;
    fn kind(&self) -> WebhookReceiverKind;
    fn icon_svg(&self) -> Option<&str> {
        None
    }
    fn receive(
        &self,
        ctx: WebhookReceiveContext,
    ) -> Result<Option<WebhookDelivery>, WebhookReceiveError>;
}

pub type WebhookRegistry = CapabilityRegistry<dyn WebhookReceiver>;
pub type WebhookRegistryHandle = Arc<WebhookRegistry>;

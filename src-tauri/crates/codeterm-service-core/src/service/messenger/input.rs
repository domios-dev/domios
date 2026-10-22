use serde::{Deserialize, Serialize};

use super::Messenger;

/// A host frame can only be minted by the messenger.
/// ```compile_fail
/// use codeterm_service_core::service::messenger::FramedDelivery;
/// let frame = FramedDelivery { opening_tag: String::new(), body: String::new(), suffix: String::new() };
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "transport", rename = "routed_frame")]
pub struct FramedDelivery {
    opening_tag: String,
    body: String,
    suffix: String,
}

impl FramedDelivery {
    pub fn to_frame(&self) -> codeterm_wire::domios::DomiosFrame {
        codeterm_wire::domios::DomiosFrame {
            opening_tag: self.opening_tag.clone(),
            body: self.body.clone(),
            suffix: self.suffix.clone(),
        }
    }

    pub fn to_message(&self) -> String {
        self.to_frame().to_message()
    }

    pub(crate) fn with_body(mut self, body: String) -> Self {
        self.body = body;
        self
    }

    pub fn body(&self) -> &str {
        &self.body
    }
}

impl Messenger {
    pub fn framed_delivery(&self, body: &str) -> FramedDelivery {
        let frame = self.routed_frame(body);
        FramedDelivery {
            opening_tag: frame.opening_tag,
            body: frame.body,
            suffix: frame.suffix,
        }
    }

    pub(crate) fn restore_framed_delivery(text: &str) -> Result<FramedDelivery, String> {
        if !text.starts_with("<domios ") {
            return Err("persisted agent delivery has no leading frame".into());
        }
        let parsed = codeterm_wire::domios::parse_message(text)
            .ok_or("persisted agent delivery has no valid frame")?;
        let opening_end = text
            .find('>')
            .ok_or("persisted agent frame has no opening tag")?
            + 1;
        Ok(FramedDelivery {
            opening_tag: text[..opening_end].into(),
            body: parsed.body.clone(),
            suffix: parsed.content[parsed.body.len()..].to_string(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct HumanInput(String);

impl HumanInput {
    pub(crate) fn into_string(self) -> String {
        self.0
    }

    pub(crate) fn text(&self) -> &str {
        &self.0
    }

    pub fn composer(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    pub fn keystrokes(text: impl Into<String>) -> Self {
        Self(text.into())
    }
}

#[derive(Debug, Clone)]
pub struct LaunchBootstrap(String);

impl LaunchBootstrap {
    pub(crate) fn text(&self) -> &str {
        &self.0
    }

    pub fn new(text: impl Into<String>) -> Self {
        Self(text.into())
    }
}

/// ```compile_fail
/// use codeterm_service_core::service::ServiceContext;
/// fn unframed(svc: &ServiceContext) { svc.send_text("target", "opaque raw text").unwrap(); }
/// ```
#[derive(Debug, Clone)]
pub enum AgentInput {
    Framed(FramedDelivery),
    Human(HumanInput),
    Launch(LaunchBootstrap),
    Shell(ShellInput),
}

impl AgentInput {
    pub(crate) fn text(&self) -> std::borrow::Cow<'_, str> {
        match self {
            Self::Framed(frame) => frame.to_message().into(),
            Self::Human(input) => input.0.as_str().into(),
            Self::Launch(input) => input.0.as_str().into(),
            Self::Shell(input) => input.0.as_str().into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct HumanKeys(Vec<u8>);

impl HumanKeys {
    pub fn new(bytes: impl AsRef<[u8]>) -> Self {
        Self(bytes.as_ref().to_vec())
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }

    pub(crate) fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

#[derive(Debug, Clone)]
pub struct ShellInput(String);

impl crate::service::ServiceContext {
    pub(crate) fn deliver_framed_input(
        &self,
        tab_id: &str,
        frame: FramedDelivery,
    ) -> Result<(), String> {
        if self.tab_chat_backing(tab_id).is_some() {
            return self
                .chatbackend_tab_send(tab_id, &AgentInput::Framed(frame))
                .or_else(|error| {
                    if crate::service::delivery_error::DeliveryError::from_opaque(
                        tab_id,
                        error.clone(),
                    )
                    .bytes_written()
                    {
                        log::warn!("[framed-input] tab={tab_id} state=submit_unconfirmed");
                        Ok(())
                    } else {
                        Err(error)
                    }
                });
        }
        let text = frame.to_message();
        self.deliver_agent_payload(
            tab_id,
            super::AgentDeliveryPayload {
                durable_delivery: None,
                trace_id: None,
                transport: frame,
                observer: None,
                idempotency_key: None,
                expected_pty_id: None,
                expected_session_id: None,
                expected_binding_nonce: None,
            },
            &text,
        )
        .map(|_| ())
    }

    pub fn programmatic_input(&self, tab_id: &str, body: &str) -> AgentInput {
        if self.tab_has_agent_identity(tab_id) || self.tab_chat_backing(tab_id).is_some() {
            AgentInput::Framed(
                Messenger::from(super::MessageEnvelope::default()).framed_delivery(body),
            )
        } else {
            AgentInput::Shell(ShellInput(body.into()))
        }
    }
}

impl<'de> Deserialize<'de> for FramedDelivery {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(tag = "transport", rename = "routed_frame")]
        struct StoredFrame {
            opening_tag: String,
            body: String,
            suffix: String,
        }
        let stored = StoredFrame::deserialize(deserializer)?;
        let frame = Self {
            opening_tag: stored.opening_tag,
            body: stored.body,
            suffix: stored.suffix,
        };
        if !frame.opening_tag.starts_with("<domios ") {
            return Err(<D::Error as serde::de::Error>::custom(
                "stored agent payload has no leading frame",
            ));
        }
        codeterm_wire::domios::parse_message(&frame.to_message()).ok_or_else(|| {
            <D::Error as serde::de::Error>::custom("stored agent payload has no valid frame")
        })?;
        Ok(frame)
    }
}

#[cfg(test)]
mod tests;

use crate::{permissions::Permission, receive::GatewayReceipt, wire::Kind, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub const REFUSAL_BODY: &str =
    "Sorry, that request is not allowed by the permissions for this Neighbor.";

#[async_trait]
pub trait RequestPorts: Send + Sync {
    async fn local_turn(&self) -> Result<GatewayReceipt>;
    async fn refusal(&self) -> Result<GatewayReceipt>;
}

pub async fn route_request(
    permission: &Permission,
    kind: Kind,
    paused: bool,
    ports: &dyn RequestPorts,
) -> Result<GatewayReceipt> {
    if matches!(kind, Kind::Answer | Kind::Refusal) {
        return Ok(GatewayReceipt::Delivered);
    }
    if !paused && permission.allows(kind) {
        ports.local_turn().await
    } else {
        ports.refusal().await
    }
}

#[derive(Deserialize, Serialize)]
pub struct Admission {
    pub message_id: String,
    pub tab: String,
    pub state: AdmissionState,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum AdmissionState {
    Claimed,
    Pending { delivery_id: String },
    Delivered,
    SubmitUnconfirmed,
    PrewriteRefused,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Resume {
    Admit,
    Delivered,
    Wait(String),
    Unconfirmed,
    WrongGateway,
}

pub fn resume(previous: Option<&Admission>, message_id: &str, tab: &str) -> Resume {
    let Some(previous) = previous.filter(|r| r.message_id == message_id) else {
        return Resume::Admit;
    };
    if previous.tab != tab {
        return Resume::WrongGateway;
    }
    match &previous.state {
        AdmissionState::Delivered => Resume::Delivered,
        AdmissionState::Pending { delivery_id } => Resume::Wait(delivery_id.clone()),
        AdmissionState::PrewriteRefused => Resume::Admit,
        AdmissionState::Claimed | AdmissionState::SubmitUnconfirmed => Resume::Unconfirmed,
    }
}

#[cfg(test)]
mod tests;

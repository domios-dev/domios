use serde::{Deserialize, Serialize};

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

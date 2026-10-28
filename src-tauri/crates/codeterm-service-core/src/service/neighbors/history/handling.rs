use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum State {
    DeliveredToConcierge,
    Working,
    Answered,
    Refused,
    NoReply,
    Failed,
    Waiting,
    Stopped,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(in crate::service::neighbors) struct Owner {
    pub tab: String,
    pub session: String,
    pub nonce: String,
    pub runtime: bool,
    pub turn: Option<String>,
    pub edge: u64,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(in crate::service::neighbors) struct Handling {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handling_state: Option<State>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handling_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handling_updated_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handling_owner: Option<Owner>,
}

impl Handling {
    pub(in crate::service::neighbors) fn active(&self) -> bool {
        matches!(
            self.handling_state,
            Some(State::Waiting | State::DeliveredToConcierge | State::Working)
        )
    }
    pub(in crate::service::neighbors) fn set(
        &mut self,
        state: State,
        reason: Option<&str>,
        at: u64,
    ) -> bool {
        if self.handling_state == Some(State::Stopped)
            || self.handling_state == Some(state) && self.handling_reason.as_deref() == reason
        {
            return false;
        }
        self.handling_state = Some(state);
        self.handling_reason = reason.map(str::to_owned);
        self.handling_updated_at = Some(at.max(self.handling_updated_at.unwrap_or(0)));
        true
    }
    pub(in crate::service::neighbors) fn outcome(&mut self, outcome: Option<&str>, at: u64) {
        match outcome {
            Some("answered") => {
                self.set(State::Answered, None, at);
            }
            Some("refused") => {
                self.set(State::Refused, None, at);
            }
            None if matches!(self.handling_state, Some(State::Answered | State::Refused)) => {
                self.set(State::Failed, Some("reply_not_delivered"), at);
            }
            _ => {}
        }
    }
}

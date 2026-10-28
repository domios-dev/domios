use crate::{engine::Engine, Consent};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Withdrawal {
    pub uid: String,
    pub device_id: Option<Uuid>,
    pub change_id: Uuid,
}

#[derive(Clone, Debug)]
pub struct ConsentTicket {
    pub sequence: u64,
    pub uid: Option<String>,
    pub device_id: Option<Uuid>,
    pub consent: Consent,
    pub withdrawals: Vec<Withdrawal>,
}

pub(crate) enum Control {
    Begin(Option<Consent>),
    Account(Option<String>),
    Approve(ConsentTicket),
    Refuse(ConsentTicket),
    Acknowledge(Uuid),
    Inspect,
}

impl Engine {
    pub(crate) fn ticket(&self) -> ConsentTicket {
        ConsentTicket {
            sequence: self.state.intent_sequence,
            uid: self.state.account_uid.clone(),
            device_id: self.state.device_id,
            consent: self.state.consent,
            withdrawals: self.state.withdrawals.clone(),
        }
    }

    fn matches(&self, ticket: &ConsentTicket) -> bool {
        self.state.intent_sequence == ticket.sequence && self.state.account_uid == ticket.uid
    }

    pub(crate) fn control(&mut self, control: Control) -> Result<ConsentTicket, String> {
        match control {
            Control::Inspect => return Ok(self.ticket()),
            Control::Approve(ticket) => {
                if !self.matches(&ticket) || !self.state.withdrawals.is_empty() {
                    return Err("Analytics intent superseded".into());
                }
                self.authorized = true;
                self.identity(ticket.uid)?;
                return Ok(self.ticket());
            }
            Control::Refuse(ticket) => {
                if !self.matches(&ticket) {
                    return Err("Analytics intent superseded".into());
                }
                self.authorized = false;
                self.consent(Consent::Off)?;
                return Ok(self.ticket());
            }
            Control::Acknowledge(change) => {
                let mut next = self.state.clone();
                next.withdrawals.retain(|intent| intent.change_id != change);
                self.commit(next)?;
                return Ok(self.ticket());
            }
            Control::Begin(intent) => {
                self.authorized = false;
                let mut next = self.state.clone();
                next.intent_sequence = next
                    .intent_sequence
                    .checked_add(1)
                    .ok_or("Analytics intent sequence exhausted")?;
                next.queue.clear();
                next.user_id = None;
                if intent == Some(Consent::Off) {
                    if let Some(uid) = next.account_uid.clone() {
                        if !next.withdrawals.iter().any(|intent| intent.uid == uid) {
                            next.withdrawals.push(Withdrawal {
                                uid,
                                device_id: next.device_id,
                                change_id: Uuid::new_v4(),
                            });
                        }
                    }
                    next.clear();
                } else if intent == Some(Consent::On) {
                    next.consent = Consent::On;
                    next.device_id.get_or_insert_with(Uuid::new_v4);
                }
                self.commit(next)?;
            }
            Control::Account(uid) => {
                self.authorized = false;
                let mut next = self.state.clone();
                next.intent_sequence = next
                    .intent_sequence
                    .checked_add(1)
                    .ok_or("Analytics intent sequence exhausted")?;
                next.account_uid = uid.filter(|value| crate::state::valid_uid(value));
                next.user_id = None;
                next.queue.clear();
                self.commit(next)?;
            }
        }
        Ok(self.ticket())
    }
}

use super::*;
use codeterm_neighbors::adapter::ReceiverStatus;
pub(super) mod friends;
pub(super) mod selection;
mod startup;
mod stop;

impl ServiceContext {
    pub(super) fn receiver_view(status: &ReceiverStatus) -> Value {
        json!({"receiving_here":status.receiving_here(), "receiver_machine_label":status.receiver_machine_label,
            "receiving_enabled":status.receiving_enabled,"revision":status.receiver_revision,"is_me":status.is_me})
    }
}

#[cfg(test)]
mod tests;

pub use codeterm_wire::api_contracts::DelayedMessageState;
pub use codeterm_wire::delayed::{DelayedMessage, DelayedMessageView};

/// Who queued a reminder, resolved from the caller tab at enqueue time.
#[derive(Debug, Default, Clone)]
pub struct ReminderCreator {
    pub tab: Option<String>,
    pub label: Option<String>,
    pub model: Option<String>,
}

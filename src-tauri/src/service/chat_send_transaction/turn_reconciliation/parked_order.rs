use super::ChatSendState;

pub(super) fn blocks_later(same_launch: bool, earlier: bool, state: &ChatSendState) -> bool {
    same_launch && earlier && *state == ChatSendState::Written
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parked_user_delivery_holds_later_parked_input_until_prior_written_resolves() {
        assert!(blocks_later(true, true, &ChatSendState::Written));
        for state in [
            ChatSendState::Sent,
            ChatSendState::Unknown,
            ChatSendState::Pending,
            ChatSendState::Failed,
        ] {
            assert!(!blocks_later(true, true, &state));
        }
    }

    #[test]
    fn parked_user_delivery_order_gate_ignores_foreign_launch_and_later_sequence() {
        assert!(!blocks_later(false, true, &ChatSendState::Written));
        assert!(!blocks_later(true, false, &ChatSendState::Written));
    }
}

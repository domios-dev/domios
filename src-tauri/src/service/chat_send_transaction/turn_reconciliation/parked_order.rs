use super::ChatSendState;

pub(super) fn blocks_later(
    held: bool,
    same_launch: bool,
    earlier: bool,
    state: &ChatSendState,
) -> bool {
    held && same_launch && earlier && *state == ChatSendState::Written
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parked_user_delivery_holds_later_parked_input_until_prior_written_resolves() {
        assert!(blocks_later(true, true, true, &ChatSendState::Written));
        for state in [
            ChatSendState::Sent,
            ChatSendState::Unknown,
            ChatSendState::Pending,
            ChatSendState::Failed,
        ] {
            assert!(!blocks_later(true, true, true, &state));
        }
    }

    #[test]
    fn parked_user_delivery_order_gate_ignores_post_ready_foreign_and_later_sends() {
        assert!(!blocks_later(false, true, true, &ChatSendState::Written));
        assert!(!blocks_later(true, false, true, &ChatSendState::Written));
        assert!(!blocks_later(true, true, false, &ChatSendState::Written));
    }
}

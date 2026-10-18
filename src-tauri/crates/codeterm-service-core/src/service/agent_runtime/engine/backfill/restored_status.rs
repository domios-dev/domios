use codeterm_plugin::agent_runtime::RuntimeTurnState;
use codeterm_types::agent::chat_types::ChatMsg;

pub(super) fn history_state(items: &[ChatMsg], has_open_turn: bool) -> Option<RuntimeTurnState> {
    if has_open_turn {
        return Some(RuntimeTurnState::Working);
    }
    items
        .iter()
        .max_by_key(|item| item.seq)
        .filter(|item| item.kind == "assistant")
        .map(|_| RuntimeTurnState::Idle)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(kind: &str, seq: i64) -> ChatMsg {
        ChatMsg {
            kind: kind.into(),
            seq,
            ..Default::default()
        }
    }

    #[test]
    fn completed_own_session_reply_is_idle_regardless_of_page_order() {
        for items in [
            vec![row("user", 1), row("assistant", 2)],
            vec![row("assistant", 2), row("user", 1)],
        ] {
            assert_eq!(history_state(&items, false), Some(RuntimeTurnState::Idle));
        }
    }

    #[test]
    fn missing_or_unfinished_evidence_stays_unknown() {
        for items in [
            vec![],
            vec![row("user", 1)],
            vec![row("assistant", 1), row("user", 2)],
            vec![row("tool_call", 3)],
        ] {
            assert_eq!(history_state(&items, false), None);
        }
    }

    #[test]
    fn open_turn_outranks_a_previous_completed_reply() {
        assert_eq!(
            history_state(&[row("assistant", 2)], true),
            Some(RuntimeTurnState::Working)
        );
    }
}

use super::{note, LaunchEvidence};

pub(crate) fn note_first_turn_input_ready(tab_id: &str, pty_id: &str, binding_nonce: &str) -> bool {
    note(
        tab_id,
        pty_id,
        LaunchEvidence::FirstTurnInputReady,
        |attempt| attempt.binding_nonce == binding_nonce,
    )
}

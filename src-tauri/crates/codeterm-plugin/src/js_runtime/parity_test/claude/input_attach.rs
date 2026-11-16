use super::*;
use crate::provider::InputAttachSignal;

#[test]
fn parity_claude_declares_session_start_and_bracketed_paste_input_attach() {
    if require_plugins().is_none() {
        return;
    }
    let (native, js) = pair();
    assert_eq!(
        native.input_attach_signal(),
        InputAttachSignal::SessionStartAndBracketedPaste
    );
    assert_eq!(js.input_attach_signal(), native.input_attach_signal());
}

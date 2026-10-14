#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OrderedPtyAction {
    Submit,
    Raw(Vec<u8>),
    Delay(std::time::Duration),
}

pub(crate) fn await_action_deadline(delay: std::time::Duration) {
    let gate = parking_lot::Mutex::new(());
    let changed = parking_lot::Condvar::new();
    let mut guard = gate.lock();
    let _ = changed.wait_for(&mut guard, delay);
}

pub(crate) fn execute_ordered_actions<E>(
    text: &str,
    actions: &[OrderedPtyAction],
    mut submit: impl FnMut(&str, bool) -> Result<(), E>,
    mut raw: impl FnMut(&[u8]) -> Result<(), E>,
    mut await_deadline: impl FnMut(std::time::Duration),
) -> Result<(), E> {
    let has_submit = actions
        .iter()
        .any(|action| matches!(action, OrderedPtyAction::Submit));
    let mut text_pending = !text.is_empty();
    if !has_submit && text_pending {
        submit(text, false)?;
        text_pending = false;
    }

    for action in actions {
        match action {
            OrderedPtyAction::Delay(delay) => await_deadline(*delay),
            OrderedPtyAction::Raw(bytes) => raw(bytes)?,
            OrderedPtyAction::Submit => {
                let body = if text_pending { text } else { "" };
                submit(body, true)?;
                text_pending = false;
            }
        }
    }
    Ok(())
}

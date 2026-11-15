use super::*;

#[test]
fn the_writer_flushes_a_failed_blur_release_and_discards_queued_rendered_frames() {
    let mut state = OutputWindowState {
        active: false,
        release_pending: true,
    };
    assert_eq!(state.before_write(true), (true, false));
    assert_eq!(state.before_write(false), (false, true));
    state.active = true;
    assert_eq!(state.before_write(true), (false, true));
}

#[test]
fn failed_blur_release_precedes_the_next_successful_send() {
    let mut state = OutputWindowState::default();
    assert!(state
        .send(NativeOutputFlow::DetachAll, |_| Err("QueueFull".into()))
        .is_err());
    assert!(state.release_pending);
    let mut sent = Vec::new();
    state
        .send(
            NativeOutputFlow::Detach {
                pty_id: "pty".into(),
                attachment: uuid::Uuid::new_v4(),
            },
            |update| {
                sent.push(update);
                Ok(())
            },
        )
        .unwrap();
    assert!(matches!(
        sent.as_slice(),
        [NativeOutputFlow::DetachAll, NativeOutputFlow::Detach { .. }]
    ));
    assert!(!state.release_pending);
}

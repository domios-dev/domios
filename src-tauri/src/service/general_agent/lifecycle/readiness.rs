/// Wait for agent output on the screen-change boundary until the failure deadline.
pub(super) fn await_agent_output(
    screen_state: &crate::ScreenStateHandle,
    registry: &crate::agent::registry::AgentRegistry,
    pty_id: &str,
    timeout: std::time::Duration,
) -> bool {
    let subscription = ScreenChangeSubscription::arm(screen_state, pty_id);
    await_agent_output_with_signal(
        screen_state,
        registry,
        pty_id,
        &subscription.signal,
        timeout,
        || false,
        || {},
    )
}

pub(super) fn await_agent_launch_evidence(
    screen_state: &crate::ScreenStateHandle,
    registry: &crate::agent::registry::AgentRegistry,
    mux: &crate::MuxState,
    tab_id: &str,
    pty_id: &str,
    timeout: std::time::Duration,
) -> bool {
    let subscription = ScreenChangeSubscription::arm(screen_state, pty_id);
    await_agent_output_with_signal(
        screen_state,
        registry,
        pty_id,
        &subscription.signal,
        timeout,
        || {
            mux.read().get_tab(tab_id).is_some_and(|tab| {
                tab.session_binding_nonce.is_some()
                    && tab.session_binding_nonce == tab.reported_session_binding_nonce
            })
        },
        || {},
    )
}

pub(super) fn await_agent_output_with_signal(
    screen_state: &crate::ScreenStateHandle,
    registry: &crate::agent::registry::AgentRegistry,
    pty_id: &str,
    signal: &codeterm_pty::pty::screen_state::ScreenChangeSignal,
    timeout: std::time::Duration,
    has_authoritative_binding: impl Fn() -> bool,
    on_wait_armed: impl FnOnce(),
) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    let mut generation = signal.generation();
    let mut on_wait_armed = Some(on_wait_armed);
    loop {
        let screen = screen_state.lock().get_screen_text(pty_id);
        let detected = screen.is_some_and(|screen| registry.detect_from_output(&screen).is_some());
        if detected || has_authoritative_binding() {
            return true;
        }
        let Some(remaining) = deadline.checked_duration_since(std::time::Instant::now()) else {
            return false;
        };
        if let Some(on_wait_armed) = on_wait_armed.take() {
            on_wait_armed();
        }
        if !signal.wait_after(generation, remaining) {
            return false;
        }
        generation = signal.generation();
    }
}

pub(super) struct ScreenChangeSubscription {
    screen_state: crate::ScreenStateHandle,
    pty_id: String,
    pub(super) signal: std::sync::Arc<codeterm_pty::pty::screen_state::ScreenChangeSignal>,
}

impl ScreenChangeSubscription {
    pub(super) fn arm(screen_state: &crate::ScreenStateHandle, pty_id: &str) -> Self {
        let signal = screen_state.lock().subscribe_screen_changes(pty_id);
        Self {
            screen_state: screen_state.clone(),
            pty_id: pty_id.to_string(),
            signal,
        }
    }
}

impl Drop for ScreenChangeSubscription {
    fn drop(&mut self) {
        self.screen_state
            .lock()
            .unsubscribe_screen_changes(&self.pty_id, &self.signal);
    }
}

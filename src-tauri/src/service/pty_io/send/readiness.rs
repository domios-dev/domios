use crate::service::ServiceContext;

impl ServiceContext {
    pub(crate) fn await_agent_tui_frame_on_screen(
        &self,
        pty_id: &str,
        agent_id: &str,
        timeout: std::time::Duration,
    ) -> bool {
        self.await_agent_tui_screen(pty_id, agent_id, timeout, false)
    }

    pub(crate) fn await_agent_tui_on_screen(
        &self,
        pty_id: &str,
        agent_id: &str,
        timeout: std::time::Duration,
    ) -> bool {
        self.await_agent_tui_screen(pty_id, agent_id, timeout, true)
    }

    pub(crate) fn await_provider_readiness_on_screen(
        &self,
        pty_id: &str,
        agent_id: &str,
        strategy: crate::agent::provider::ProviderReadinessStrategy,
    ) -> bool {
        self.await_agent_tui_screen(
            pty_id,
            agent_id,
            strategy.timeout(),
            strategy.requires_composer(),
        )
    }

    fn await_agent_tui_screen(
        &self,
        pty_id: &str,
        agent_id: &str,
        timeout: std::time::Duration,
        require_composer: bool,
    ) -> bool {
        let signal = self.screen_state.lock().arm_screen_change_signal(pty_id);
        let deadline = std::time::Instant::now() + timeout;
        let generation = std::cell::Cell::new(signal.generation());
        wait_for_observed_readiness(
            deadline,
            || {
                let (screen, alternate_screen, composer_ready) = {
                    let screen_state = self.screen_state.lock();
                    let screen = screen_state.get_screen_text(pty_id).unwrap_or_default();
                    let cursor = screen_state.cursor_input_state(pty_id);
                    let (alternate_screen, composer_ready) = cursor
                        .map(|(alternate, visible, row, rows)| {
                            (
                                alternate,
                                crate::agent::offline_state_classifier::composer_cursor_ready(
                                    visible, row, rows,
                                ),
                            )
                        })
                        .unwrap_or((false, false));
                    (screen, alternate_screen, composer_ready)
                };
                let provider_ready = self
                    .agent_registry
                    .with_provider(agent_id, |provider| provider.screen_has_tui(&screen));
                let provider_prompt = self
                    .agent_registry
                    .parse_prompt(agent_id, &screen)
                    .is_some();
                let ready = provider_or_generic_tui(
                    provider_ready,
                    codeterm_pty::pty::screen_state::screen_has_agent_tui(&screen),
                );
                provider_frame_ready(
                    ready,
                    provider_prompt,
                    require_composer,
                    alternate_screen,
                    composer_ready,
                )
            },
            |remaining| {
                let changed = signal.wait_after(generation.get(), remaining);
                generation.set(signal.generation());
                changed
            },
        )
    }
}

pub(crate) fn provider_or_generic_tui(provider_ready: Option<bool>, generic_ready: bool) -> bool {
    provider_ready.unwrap_or(generic_ready)
}

pub(super) fn wait_for_observed_readiness(
    deadline: std::time::Instant,
    mut observed: impl FnMut() -> bool,
    mut wait_change: impl FnMut(std::time::Duration) -> bool,
) -> bool {
    loop {
        if observed() {
            return true;
        }
        let Some(remaining) = deadline.checked_duration_since(std::time::Instant::now()) else {
            return false;
        };
        if remaining.is_zero() {
            return false;
        }
        if !wait_change(remaining) {
            return false;
        }
    }
}

pub(super) fn provider_frame_ready(
    tui_ready: bool,
    prompt_ready: bool,
    require_composer: bool,
    alternate_screen: bool,
    composer_ready: bool,
) -> bool {
    let structural_tui_ready = tui_ready || (alternate_screen && composer_ready);
    (structural_tui_ready && (!require_composer || composer_ready))
        || (!require_composer && prompt_ready)
}

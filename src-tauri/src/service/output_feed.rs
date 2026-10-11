use super::ServiceContext;
use crate::agent::status::TabStatus;
use tauri::Manager;

impl ServiceContext {
    #[cfg(debug_assertions)]
    pub(crate) fn feed_pty_output(&self, pty_id: &str, data: &[u8]) -> (TabStatus, TabStatus) {
        self.feed_pty_output_inner(pty_id, None, data)
    }

    #[cfg(test)]
    pub(crate) fn feed_pty_output_for_generation(
        &self,
        pty_id: &str,
        generation: u64,
        data: &[u8],
    ) -> (TabStatus, TabStatus) {
        self.feed_pty_output_inner(pty_id, Some(generation), data)
    }

    fn feed_pty_output_inner(
        &self,
        pty_id: &str,
        generation: Option<u64>,
        data: &[u8],
    ) -> (TabStatus, TabStatus) {
        if self.prepare_pty_output(pty_id, generation, data) {
            self.finish_pty_output(pty_id, generation, data)
        } else {
            let status = self.agent_tracker.lock().get_status(pty_id);
            (status, status)
        }
    }

    pub(super) fn prepare_pty_output(
        &self,
        pty_id: &str,
        generation: Option<u64>,
        data: &[u8],
    ) -> bool {
        if data.is_empty() {
            return false;
        }
        self.output_buf.lock().append(pty_id, data);
        self.output_stream_pty_changed(pty_id);
        let accepted = match generation {
            Some(generation) => self
                .screen_state
                .lock()
                .process_if_generation(pty_id, generation, data),
            None => {
                self.screen_state.lock().process(pty_id, data);
                true
            }
        };
        super::pty_frame_timing::parsed(pty_id, data.len(), accepted);
        accepted
    }

    pub(super) fn finish_pty_output(
        &self,
        pty_id: &str,
        generation: Option<u64>,
        data: &[u8],
    ) -> (TabStatus, TabStatus) {
        if generation.is_some_and(|generation| {
            self.screen_state.lock().pty_generation(pty_id) != Some(generation)
        }) {
            let status = self.agent_tracker.lock().get_status(pty_id);
            return (status, status);
        }
        let trace = std::env::var("CODETERM_RD_TRACE").is_ok();
        let observed_tab = { self.mux.read().tab_id_for_pty(pty_id) };
        if let Some(tab_id) = observed_tab {
            super::supporter_tab_state::note_output(&tab_id);
            if let Some(handle) = self
                .app_handle
                .try_state::<crate::sentinel::scheduler::Handle>()
            {
                handle.terminal_output(&tab_id, generation, data);
            }
        }
        crate::service::pty_io::submit_transaction::wake_submit_cycle_output(pty_id);
        let stage = std::time::Instant::now();
        let structured = self
            .mux
            .read()
            .tab_id_for_pty(pty_id)
            .is_some_and(|tab| super::codex_rpc::lifecycle::for_tab(&tab).is_some());
        if !structured {
            self.run_watcher.feed(self, pty_id, data);
        }
        let watcher_us = stage.elapsed().as_micros();
        let stage = std::time::Instant::now();
        let statuses = self.process_pty_output(pty_id, data);
        let changed_tab = self.mux.read().tab_id_for_pty(pty_id);
        if let Some(tab_id) = changed_tab {
            self.wake_agent_deliveries_on_state_change(&tab_id);
            self.wake_user_owner_on_state_change(&tab_id);
        }
        crate::service::pty_io::submit_transaction::notify_submit_cycle_status_transition(
            pty_id, statuses.0, statuses.1,
        );
        let status_us = stage.elapsed().as_micros();
        if trace && data.len() > 16 {
            log::info!(
                "[feed-trace] pty={} n={} watcher={}us status={}us",
                &pty_id[..pty_id.len().min(8)],
                data.len(),
                watcher_us,
                status_us,
            );
        }
        statuses
    }

    pub(crate) fn emit_tab_status_if_changed(&self, pty_id: &str, prev: TabStatus, now: TabStatus) {
        self.publish_authoritative_tab_statuses();
        if now == prev {
            return;
        }
        self.output_stream_pty_changed(pty_id);
        let tab_id = self.mux.read().tab_id_for_pty(pty_id);
        if let Some(ref pid) = tab_id {
            if now == TabStatus::Idle {
                self.apply_staged_model_switch_on_idle(pid);
            }
            self.wake_agent_deliveries_on_state_change(pid);
            self.wake_user_owner_on_state_change(pid);
        }
    }

    pub(crate) fn mark_agent_turn_error(&self, tab_id: &str) -> bool {
        if self.is_agent_stop_suppressed_for_tab(tab_id) {
            log::info!("[tab-stop] suppressed_turn_error tab={}", tab_id);
            return self.mark_agent_turn_complete(tab_id);
        }
        self.mark_agent_turn_status(tab_id, TabStatus::Error)
    }

    pub(crate) fn mark_agent_turn_complete(&self, tab_id: &str) -> bool {
        self.mark_agent_turn_status(tab_id, TabStatus::Idle)
    }

    pub(crate) fn mark_agent_waiting(&self, tab_id: &str) -> bool {
        self.mark_agent_turn_status(tab_id, TabStatus::Waiting)
    }

    pub(crate) fn mark_agent_turn_started(&self, tab_id: &str) -> bool {
        self.staged_model_turn_started(tab_id);
        self.mark_agent_turn_status(tab_id, TabStatus::Working)
    }

    fn mark_agent_turn_status(&self, tab_id: &str, status: TabStatus) -> bool {
        let pty_id = self
            .mux
            .read()
            .get_tab(tab_id)
            .and_then(|tab| tab.pty_id.clone());
        let Some(pty_id) = pty_id else {
            return false;
        };
        let (previous, current) = {
            let mut tracker = self.agent_tracker.lock();
            let previous = tracker.get_status(&pty_id);
            match status {
                TabStatus::Working => tracker.set_working(&pty_id),
                TabStatus::Waiting => tracker.set_waiting(&pty_id),
                TabStatus::Error => tracker.set_error(&pty_id),
                TabStatus::Idle => tracker.set_idle(&pty_id),
                _ => return false,
            }
            if matches!(status, TabStatus::Idle | TabStatus::Error) {
                tracker.notify_terminal_event(&pty_id);
            }
            (previous, tracker.get_status(&pty_id))
        };
        crate::agent::live_state::schedule(self, &pty_id);
        self.emit_tab_status_if_changed(&pty_id, previous, current);
        crate::service::pty_io::submit_transaction::notify_submit_cycle_status_transition(
            &pty_id, previous, current,
        );
        self.emit_state_changed();
        true
    }
}

use super::ServiceContext;
use crate::agent::status::TabStatus;

impl ServiceContext {
    #[cfg(debug_assertions)]
    pub(crate) fn feed_pty_output(&self, pty_id: &str, data: &[u8]) -> (TabStatus, TabStatus) {
        self.feed_pty_output_inner(pty_id, None, data)
    }

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
        if data.is_empty() {
            let status = self.agent_tracker.lock().get_status(pty_id);
            return (status, status);
        }
        let trace = std::env::var("CODETERM_RD_TRACE").is_ok();
        let stage = std::time::Instant::now();
        self.output_buf.lock().append(pty_id, data);
        self.output_stream_pty_changed(pty_id);
        let output_us = stage.elapsed().as_micros();
        let stage = std::time::Instant::now();
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
        let screen_us = stage.elapsed().as_micros();
        if !accepted {
            let status = self.agent_tracker.lock().get_status(pty_id);
            return (status, status);
        }
        crate::service::pty_io::submit_transaction::wake_submit_cycle_output(pty_id);
        let stage = std::time::Instant::now();
        self.run_watcher.feed(self, pty_id, data);
        let watcher_us = stage.elapsed().as_micros();
        let stage = std::time::Instant::now();
        let statuses = self.process_pty_output(pty_id, data);
        if statuses.0 != statuses.1 && statuses.1 == TabStatus::Idle {
            crate::service::pty_io::submit_transaction::wake_submit_cycle_status(pty_id);
        }
        let status_us = stage.elapsed().as_micros();
        if trace && data.len() > 16 {
            log::info!(
                "[feed-trace] pty={} n={} output={}us screen={}us watcher={}us status={}us",
                &pty_id[..pty_id.len().min(8)],
                data.len(),
                output_us,
                screen_us,
                watcher_us,
                status_us,
            );
        }
        statuses
    }

    pub(crate) fn emit_tab_status_if_changed(&self, pty_id: &str, prev: TabStatus, now: TabStatus) {
        if now == prev {
            return;
        }
        self.output_stream_pty_changed(pty_id);
        let tab_id = self.mux.read().tab_id_for_pty(pty_id);
        if let Some(ref pid) = tab_id {
            crate::api::events::emit_everywhere(
                &self.app_handle,
                "tab-status-changed",
                serde_json::json!({
                    "tab_id": pid,
                    "status": format!("{:?}", now),
                }),
            );
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

    pub(crate) fn mark_agent_turn_started(&self, tab_id: &str) -> bool {
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
                TabStatus::Error => tracker.set_error(&pty_id),
                TabStatus::Idle => tracker.set_idle(&pty_id),
                _ => return false,
            }
            if status != TabStatus::Working {
                tracker.notify_terminal_event(&pty_id);
            }
            (previous, tracker.get_status(&pty_id))
        };
        self.emit_tab_status_if_changed(&pty_id, previous, current);
        if status == TabStatus::Idle {
            crate::service::pty_io::submit_transaction::wake_submit_cycle_status(&pty_id);
        }
        self.emit_state_changed();
        true
    }
}

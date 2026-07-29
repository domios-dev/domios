use super::ServiceContext;
use crate::agent::status::TabStatus;

impl ServiceContext {
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
        let output_us = stage.elapsed().as_micros();
        let stage = std::time::Instant::now();
        log::debug!(
            "[screen-state-stage] pty={} waiting bytes={}",
            pty_id,
            data.len()
        );
        let accepted = {
            let mut screen_state = self.screen_state.lock();
            log::debug!(
                "[screen-state-stage] pty={} acquired bytes={}",
                pty_id,
                data.len()
            );
            let accepted = match generation {
                Some(generation) => {
                    screen_state.process_if_generation(pty_id, generation, data)
                }
                None => {
                    screen_state.process(pty_id, data);
                    true
                }
            };
            log::debug!(
                "[screen-state-stage] pty={} processed bytes={} accepted={}",
                pty_id,
                data.len(),
                accepted
            );
            accepted
        };
        let screen_us = stage.elapsed().as_micros();
        if !accepted {
            let status = self.agent_tracker.lock().get_status(pty_id);
            return (status, status);
        }
        let stage = std::time::Instant::now();
        self.run_watcher.feed(self, pty_id, data);
        let watcher_us = stage.elapsed().as_micros();
        let stage = std::time::Instant::now();
        let statuses = self.process_pty_output(pty_id, data);
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
}

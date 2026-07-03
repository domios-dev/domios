use super::ServiceContext;
use crate::agent::status::PaneStatus;

impl ServiceContext {
    pub(crate) fn feed_pty_output(&self, pty_id: &str, data: &[u8]) -> (PaneStatus, PaneStatus) {
        if data.is_empty() {
            let status = self.agent_tracker.lock().get_status(pty_id);
            return (status, status);
        }
        self.output_buf.lock().append(pty_id, data);
        self.screen_state.lock().process(pty_id, data);
        self.run_watcher.feed(self, pty_id, data);
        self.process_pty_output(pty_id, data)
    }

    pub(crate) fn emit_pane_status_if_changed(
        &self,
        pty_id: &str,
        prev: PaneStatus,
        now: PaneStatus,
    ) {
        if now == prev {
            return;
        }
        let pane_id = self.mux.read().pane_id_for_pty(pty_id);
        if let Some(ref pid) = pane_id {
            crate::api::events::emit_everywhere(
                &self.app_handle,
                "pane-status-changed",
                serde_json::json!({
                    "pane_id": pid,
                    "status": format!("{:?}", now),
                }),
            );
        }
    }
}

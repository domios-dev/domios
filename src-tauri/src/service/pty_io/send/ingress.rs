use crate::service::ServiceContext;

pub(super) fn spawn_with_submit_claim(
    pty_id: Option<String>,
    spawn: impl FnOnce(Option<super::super::submit_transaction::SubmitCycleClaim>),
) {
    let claim = pty_id
        .as_deref()
        .map(super::super::submit_transaction::claim_submit_cycle);
    spawn(claim);
}

impl ServiceContext {
    pub async fn send_text_when_ready(&self, tab_id: &str, text: &str) -> Result<(), String> {
        self.send_text_detached(tab_id, text);
        Ok(())
    }

    pub(crate) async fn stage_text_when_ready(
        &self,
        tab_id: &str,
        text: &str,
    ) -> Result<(), String> {
        self.send_text_input_detached(tab_id, text, false);
        Ok(())
    }

    pub fn write_raw(&self, tab_id: &str, data: &[u8]) -> Result<(), String> {
        let pty_id = self.resolve_pty_id(tab_id)?;
        let claim = super::super::submit_transaction::claim_submit_cycle(&pty_id);
        claim
            .enqueue_if_current(|| self.pty.lock().write(&pty_id, data))
            .unwrap_or_else(|| {
                Err(format!(
                    "agent_unavailable: PTY input superseded before enqueue: {pty_id}"
                ))
            })
    }

    pub fn inject_raw_keys_async(&self, tab_id: &str, writes: Vec<Vec<u8>>) -> Result<(), String> {
        let pty_id = self.resolve_pty_id(tab_id)?;
        let pty = self.pty.clone();
        let write_pty_id = pty_id.clone();
        let claim = super::super::submit_transaction::claim_submit_cycle(&pty_id);
        std::thread::spawn(move || {
            for bytes in writes {
                let Some(result) =
                    claim.enqueue_if_current(|| pty.lock().write(&write_pty_id, &bytes))
                else {
                    break;
                };
                if result.is_err() {
                    break;
                }
            }
        });
        Ok(())
    }

    pub fn send_text_detached(&self, tab_id: &str, text: &str) {
        self.send_text_input_detached(tab_id, text, true);
    }

    pub(crate) fn send_text_input_detached(&self, tab_id: &str, text: &str, submit: bool) {
        let svc = self.clone();
        let tab = tab_id.to_string();
        let text = text.to_string();
        let pty_id = self.resolve_pty_id(tab_id).ok();
        spawn_with_submit_claim(pty_id, move |submit_claim| {
            std::thread::spawn(move || {
                if let Err(error) =
                    svc.send_text_input_delivery_claimed(&tab, &text, submit, submit_claim)
                {
                    let error = error.ledger_message();
                    if super::delivery_is_unknown(&error) {
                        log::warn!("[send-async] delivery to Tab {tab} remains unknown");
                        return;
                    }
                    log::warn!("[send-async] delivery to Tab {tab} failed: {error}");
                    crate::api::events::emit_everywhere(
                        &svc.app_handle,
                        "chat-send-failed",
                        serde_json::json!({"tab_id": &tab, "error": &error}),
                    );
                }
            });
        });
    }

    pub(crate) fn feed_input_parser(&self, pty_id: &str, text: &str, enter: bool) {
        let (tab_id, locked) = {
            let mux = self.mux.read();
            let tab_id = mux.tab_id_for_pty(pty_id);
            let locked = tab_id
                .as_ref()
                .and_then(|id| mux.get_tab(id))
                .map(|tab| tab.agent_type_locked)
                .unwrap_or(false);
            (tab_id, locked)
        };
        let Some(tab_id) = tab_id else { return };
        if locked {
            self.input_parser.clear(&tab_id);
            return;
        }
        let mut payload = text.as_bytes().to_vec();
        if enter {
            payload.push(b'\r');
        }
        if let Some(provider_id) = self.input_parser.process(&tab_id, &payload) {
            log::info!(
                "[input-parser] detected '{}' send in Tab {} — locking agent_type",
                provider_id,
                tab_id,
            );
            self.set_and_lock_tab_agent_type(&tab_id, provider_id);
            self.emit_state_changed();
        }
    }
}

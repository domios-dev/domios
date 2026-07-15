use crate::service::ServiceContext;

impl ServiceContext {
    pub fn send_text_batch(
        &self,
        pane_id: &str,
        batch: Vec<super::SendBatchItem>,
    ) -> Result<(), String> {
        if let Some((plugin_id, session_id)) = self.pane_chat_backing(pane_id) {
            for item in batch {
                self.chatbackend_send(&plugin_id, &session_id, &item.text)?;
            }
            return Ok(());
        }
        self.ensure_pane_pty_for_send(pane_id)?;
        self.await_agent_boot_blocking(pane_id, std::time::Duration::from_secs(90))?;
        let pty_id = self.await_pty_link_blocking(pane_id, std::time::Duration::from_secs(15))?;
        let pty = self.pty.clone();
        tokio::spawn(async move {
            for item in batch {
                if let Some(delay) = item.delay_ms {
                    tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
                }
                let text = if item.enter && !item.text.ends_with('\r') {
                    format!("{}\r", item.text)
                } else {
                    item.text
                };
                let _ = pty.lock().write(&pty_id, text.as_bytes());
            }
        });
        Ok(())
    }
}

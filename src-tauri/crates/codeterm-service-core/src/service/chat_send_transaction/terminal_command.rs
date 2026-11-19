use super::{ChatSendStatus, ExpectedChatRecipient};
use crate::service::ServiceContext;
use codeterm_types::pty::BootStage;

impl ServiceContext {
    pub fn send_composer_terminal_command(
        &self,
        tab_id: &str,
        expected: &ExpectedChatRecipient,
        client_token: &str,
        text: &str,
    ) -> Result<ChatSendStatus, String> {
        self.require_tab_input_allowed(tab_id, None)?;
        let _input = self.admit_update_input(tab_id)?;
        let _claim = self.pty_ownership.acquire(tab_id)?;
        let command = text.trim();
        if !matches!(
            command,
            "/usage"
                | "/config"
                | "/doctor"
                | "/mcp"
                | "/hooks"
                | "/theme"
                | "/permissions"
                | "/help"
        ) {
            return Err("The composer terminal command is unsupported.".into());
        }
        if let Some(previous) = self
            .chat_send_ledger
            .reconcile(tab_id)
            .into_iter()
            .find(|status| status.correlation_id.as_deref() == Some(client_token))
        {
            return Ok(previous);
        }
        let status = self.chat_send_ledger.try_start_scoped_with_normalization(
            tab_id,
            command,
            None,
            None,
            Default::default(),
        )?;
        self.chat_send_ledger
            .attach_client_token(&status.send_id, client_token.to_string());
        let result = self.enqueue_composer_terminal_command(tab_id, expected, command);
        let status = self
            .chat_send_ledger
            .finish(&status.send_id, result)
            .ok_or("admission_unknown: terminal command receipt is unavailable")?;
        self.ui.emit(
            "chat-send-status",
            serde_json::to_value(&status).unwrap_or_default(),
        );
        Ok(status)
    }

    fn enqueue_composer_terminal_command(
        &self,
        tab_id: &str,
        expected: &ExpectedChatRecipient,
        command: &str,
    ) -> Result<(), String> {
        let mut bytes = command.as_bytes().to_vec();
        bytes.push(b'\r');
        let mux = self.mux.read();
        if let Some(refusal) = mux.tab_write_lock(tab_id) {
            return Err(refusal.to_string());
        }
        let tab = mux
            .get_tab(tab_id)
            .filter(|tab| {
                matches!(tab.backing, crate::multiplexer::types::TabBacking::Pty)
                    && !tab.frozen
                    && !tab.agent_booting
                    && tab.boot_stage == Some(BootStage::AgentReady)
                    && expected.matches_exact_launch(tab)
            })
            .ok_or("The terminal command requires the same ready agent session.")?;
        let pty_id = tab
            .pty_id
            .as_deref()
            .ok_or("The agent terminal is unavailable.")?;
        let manager = self.pty.lock();
        let pty = manager
            .get_session(pty_id)
            .ok_or("The agent terminal is unavailable.")?;
        let permit = manager.issue_guarded_input_permit(pty_id)?;
        let cycle = crate::service::pty_io::submit_transaction::claim_submit_cycle(pty_id);
        if cycle.command_wait().is_some() {
            return Err("A model command owns terminal input.".into());
        }
        pty.write_queue.send_guarded(bytes, permit)
    }
}

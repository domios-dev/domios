use super::CommandWait;
use crate::service::{chat_send_transaction::ChatSendStatus, parked_user_delivery::UserWake};

impl crate::service::ServiceContext {
    pub(crate) fn hold_user_send_for_model_command(
        &self,
        tab_id: &str,
        send_id: &str,
        wait: CommandWait,
    ) -> Option<ChatSendStatus> {
        let changed = self
            .chat_send_ledger
            .set_user_pending_reason(send_id, Some("awaiting_model_command"));
        if let Some(status) = &changed {
            crate::api::events::emit_everywhere(&self.app_handle, "chat-send-status", status);
        }
        let status = changed.or_else(|| self.chat_send_ledger.status(send_id))?;
        if status.state != crate::service::chat_send_transaction::ChatSendState::Pending
            || !wait.schedule_user(send_id)
        {
            return Some(status);
        }
        let service = self.clone();
        let tab_id = tab_id.to_string();
        let send_id = send_id.to_string();
        tauri::async_runtime::spawn(async move {
            if wait.completed().await {
                service.wake_user_owner(&tab_id, UserWake::Readiness);
            } else if let Some(status) = service
                .chat_send_ledger
                .set_user_pending_reason(&send_id, Some("model_command_completion_unconfirmed"))
            {
                crate::api::events::emit_everywhere(
                    &service.app_handle,
                    "chat-send-status",
                    &status,
                );
            }
        });
        Some(status)
    }
}

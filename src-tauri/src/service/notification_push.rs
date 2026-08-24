//! Telegram push sink and the daemon-side policy that gates it.

use super::notification::{PushEnvelope, PushIntent};
use super::ServiceContext;
use crate::bridge::manager::OutboundMessage;
use crate::bridge::traits::OutboundContent;
use crate::notification::push;

impl ServiceContext {
    /// Resolve the opt-in. Without an audience the category is dropped, so an
    /// install that never opted in takes exactly the pre-existing code path.
    pub(crate) fn resolve_push_policy(&self, intent: PushIntent) -> (PushIntent, u32, Vec<i64>) {
        let telegram = crate::config::load_config().notifications.telegram;
        let audience = push::resolve_audience(&telegram);
        if audience.is_empty() {
            return (PushIntent::default(), 0, audience);
        }
        (intent, telegram.per_scope_limit, audience)
    }

    /// Best-effort fan to the explicit audience. Every send names its chat id:
    /// `user_id: None` would fall back to the bridge's last speaker.
    pub(crate) fn deliver_telegram_push(
        &self,
        audience: &[i64],
        envelope: &PushEnvelope,
    ) -> Result<(), String> {
        if audience.is_empty() {
            return Err("no explicit recipients configured".to_string());
        }
        if !self.bridge_supervisor.is_active() {
            return Err("telegram bridge is not active".to_string());
        }
        let outbound = self.bridge_outbound.lock();
        let Some(tx) = outbound.as_ref() else {
            return Err("telegram outbound channel is closed".to_string());
        };

        let rejected = audience
            .iter()
            .filter(|chat_id| {
                tx.send(OutboundMessage {
                    user_id: Some(**chat_id),
                    content: OutboundContent::Text {
                        text: envelope.text.clone(),
                    },
                    append: false,
                    buttons: None,
                })
                .is_err()
            })
            .count();

        if rejected > 0 {
            return Err(format!(
                "{rejected}/{} telegram sends rejected for {}",
                audience.len(),
                envelope.level.as_str()
            ));
        }
        Ok(())
    }

    /// UI affordance gate: a live bridge plus a non-empty inbound allow list.
    pub fn telegram_push_control_available(&self) -> bool {
        let allow_list_len = crate::config::load_config()
            .integrations
            .telegram_allowed_users
            .len();
        push::control_available(self.bridge_supervisor.is_active(), allow_list_len)
    }
}

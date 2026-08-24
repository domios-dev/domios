//! Telegram push sink and the daemon-side policy that gates it.

use super::notification::{PushEnvelope, PushIntent};
use super::ServiceContext;
use crate::bridge::manager::OutboundMessage;
use crate::bridge::traits::OutboundContent;
use crate::notification::push;
use codeterm_types::config::schema::NotificationConfig;
use codeterm_types::notification::{NotifyDestination, NotifyLevel};

/// A caller that named no level never reached the channel and still does not.
/// A level with no readable row is refused rather than widened into a pass.
fn routed_to_telegram(cfg: &NotificationConfig, declared: Option<NotifyLevel>) -> bool {
    let Some(level) = declared else {
        return false;
    };
    cfg.agent_levels
        .iter()
        .filter(|row| NotifyLevel::parse(&row.level) == Some(level))
        .any(|row| row.allows(NotifyDestination::Telegram))
}

impl ServiceContext {
    /// Resolve the opt-in. Without an audience the category is dropped, so an
    /// install that never opted in takes exactly the pre-existing code path.
    pub(crate) fn resolve_push_policy(&self, intent: PushIntent) -> (PushIntent, u32, Vec<i64>) {
        let notifications = crate::config::load_config().notifications;
        let audience = push::resolve_audience(&notifications.telegram);
        if audience.is_empty() {
            return (PushIntent::default(), 0, audience);
        }
        // The grid's Telegram column is the same standing answer the bell and
        // the toast read: a level routed away from Telegram clears its intent,
        // which is exactly the code path an install with no audience takes.
        if !routed_to_telegram(&notifications, intent.level) {
            return (PushIntent::default(), 0, Vec::new());
        }
        (intent, notifications.telegram.per_scope_limit, audience)
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

#[cfg(test)]
mod tests {
    use super::*;
    use codeterm_types::config::schema::AgentLevelChoice;

    fn config_routing(level: NotifyLevel, telegram: bool) -> NotificationConfig {
        NotificationConfig {
            agent_levels: NotifyLevel::ALL
                .into_iter()
                .map(|candidate| {
                    let mut row = AgentLevelChoice::open(candidate.as_str());
                    if candidate == level {
                        row.set(NotifyDestination::Telegram, telegram);
                    }
                    row
                })
                .collect(),
            ..NotificationConfig::default()
        }
    }

    #[test]
    fn notification_settings_telegram_column_gates_only_its_own_level() {
        let cfg = config_routing(NotifyLevel::Verbose, false);
        assert!(!routed_to_telegram(&cfg, Some(NotifyLevel::Verbose)));
        assert!(routed_to_telegram(&cfg, Some(NotifyLevel::ActionNeeded)));
        assert!(routed_to_telegram(&cfg, Some(NotifyLevel::Failure)));
    }

    #[test]
    fn notification_settings_absent_grid_routes_every_level_to_telegram() {
        let cfg = NotificationConfig::default();
        for level in NotifyLevel::ALL {
            assert!(
                routed_to_telegram(&cfg, Some(level)),
                "{} reached an opted-in audience before the grid existed",
                level.as_str()
            );
        }
        assert!(
            !routed_to_telegram(&cfg, None),
            "naming no level never reached the channel"
        );
    }

    #[test]
    fn notification_settings_level_with_no_readable_row_is_refused() {
        let cfg = NotificationConfig {
            agent_levels: vec![AgentLevelChoice::open("milestone")],
            ..NotificationConfig::default()
        };
        for level in NotifyLevel::ALL {
            assert!(!routed_to_telegram(&cfg, Some(level)));
        }
    }
}

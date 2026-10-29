use super::*;
use crate::notification::{
    channels::{self, ChannelHost},
    push::NotifyLevel,
    store::NotificationStore,
};
use codeterm_bridge::bridge::{
    manager::{create_outbound_channel, OutboundMessage, OutboundSender},
    traits::OutboundContent,
};
use codeterm_types::config::schema::{AgentLevelChoice, AppConfig, PushChannelConfig};
use parking_lot::Mutex;
use std::sync::Arc;

struct Host(OutboundSender);

impl ChannelHost for Host {
    fn service_ready(&self, service: &str) -> bool {
        service == "telegram" && self.0.lock().is_some()
    }
    fn enqueue(&self, _: &str, message: OutboundMessage) -> Result<(), String> {
        self.0
            .lock()
            .as_ref()
            .ok_or("closed")?
            .send(message)
            .map_err(|_| "closed".into())
    }
}

fn config() -> AppConfig {
    let mut config = AppConfig::default();
    config.notifications.agent_levels = vec![AgentLevelChoice::open("verbose", &["telegram"])];
    config.notifications.channels.insert(
        "telegram".into(),
        PushChannelConfig {
            recipients: vec!["100000001".into()],
            per_scope_limit: 10,
        },
    );
    config
}

#[test]
fn configured_push_connection_and_flag_notices_keep_profiles_local_and_message_push_is_generic() {
    for kind in ["request", "accepted", "flag", "message"] {
        let (outbound, mut inbox) = create_outbound_channel();
        let host = Host(outbound);
        let intent = PushIntent {
            level: Some(NotifyLevel::Verbose),
            event_key: Some(kind.into()),
        };
        let plan = crate::service::notification_push::plan_push(
            &config(),
            &channels::roster(),
            &host,
            intent.clone(),
        );
        assert_eq!(plan.targets.len(), 1);
        let title = format!("Anya (anya@example.test): {kind} private-preview");
        let notice = if matches!(kind, "request" | "accepted") {
            InAppNotice::neighbor_connection(title.clone(), kind.into())
        } else {
            InAppNotice {
                title: title.clone(),
                actions: vec![
                    codeterm_notification_core::contract::NotificationAction::Url {
                        label: "Anya private action".into(),
                        value: "domios://neighbor/opaque".into(),
                    },
                ],
                origin: NotificationOrigin::local_tab("opaque".into()),
                intent,
                include_push: kind == "message",
            }
        };
        let store = Arc::new(Mutex::new(NotificationStore::new(10)));
        let id = create(
            &store,
            notice,
            true,
            PushDispatch {
                intent: plan.intent,
                per_scope_limit: plan.per_scope_limit,
                deliver: |envelope| {
                    crate::service::notification_push::deliver_plan(
                        &plan.targets,
                        &plan.refusals,
                        &host,
                        &envelope,
                    )
                },
            },
            |_| {
                Some(NotificationProvenance {
                    tab_title: Some("Anya private concierge".into()),
                    ..Default::default()
                })
            },
            |_| {},
        );
        assert_eq!(store.lock().notification(&id).unwrap().title, title);
        if kind == "message" {
            let queued = inbox.try_recv().unwrap();
            let OutboundContent::Text { text } = queued.content else {
                panic!("expected text")
            };
            assert!(text.contains("Neighbor activity"));
            for private in [
                "Anya",
                "anya@example.test",
                "private-preview",
                "private concierge",
                "private action",
            ] {
                assert!(!text.contains(private));
            }
        } else {
            assert!(inbox.try_recv().is_err());
        }
        assert!(inbox.try_recv().is_err());
    }
}

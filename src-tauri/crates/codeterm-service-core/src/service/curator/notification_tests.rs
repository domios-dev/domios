use super::*;
use crate::notification::store::{Ask, NotificationProvenance, NotificationSeverity};
use codeterm_notification_core::contract::{NotificationAction, NotifyLevel};
use std::sync::Arc;
use parking_lot::Mutex;

#[test]
fn curator_question_resolution_schedules_next_round_and_emits_ui_changes() {
    for action in ["chat", "resolve", "clear", "tap"] {
        let (fixture, id, _, owner) = crate::service::plan_authority::test_fixture::fixture();
        let service = &fixture.service;
        let owner = owner.tab_id.unwrap();
        let now = 1_700_000_000_000;
        let clock = crate::cron::clock::MockClock::starting_at(now);
        let cron = Arc::new(Mutex::new(crate::cron::store::CronStore::new_with_path_and_clock(
            fixture.temp.path().join("crons.json"), clock)));
        assert!(service.set_ext(cron.clone()).is_ok());
        service.agent_registry.register_provider(Box::new(crate::agent::test_provider::MockProvider::new("provider-neutral")));
        service.mux.write().get_tab_mut(&owner).unwrap().agent_session_model = Some("fixture-model".into());
        service.plan_store.with_plan(&id, now, |plan| {
            plan.mode = PlanMode::Execution;
            Ok(())
        }).unwrap();
        let question = service.notif.lock().add_ext("opaque question".into(), None, NotificationSeverity::Info,
            NotificationProvenance { tab_id: Some(owner.clone()), ..Default::default() },
            Ask::offering(Some(vec![NotificationAction::Answer { label: "opaque choice".into() }])),
            None, Some(NotifyLevel::ActionNeeded));
        let parked = service.control_loop_show(&id).unwrap();
        assert_eq!(parked.suspended_reason, Some(CuratorSuspension::WaitingOnHuman));
        assert_eq!(parked.next_at, None);
        match action {
            "chat" => service.answer_notification_questions_in_chat(&owner, u64::MAX),
            "resolve" => service.resolve_notification(&question, Some(&owner)).unwrap(),
            "clear" => service.clear_notifications(None),
            "tap" => {
                service.notif.lock().record_answer(&question, "opaque choice");
                service.reconcile_notification_questions();
            }
            _ => unreachable!(),
        }
        let state = service.plan_store.get(&id).unwrap().control_loop;
        assert_eq!(state.suspended_reason, None, "{action}");
        assert_eq!(state.next_at, Some(now + curator_delay(None) * 1000), "{action}");
        let job = cron.lock().get(state.cron_id.as_ref().unwrap()).unwrap().clone();
        assert!(job.enabled);
        assert_eq!(job.next_run, state.next_at);
        if action != "tap" {
            assert!(fixture.ui.0.lock().iter().any(|(name, data)|
                (name == "notification-resolved" || name == "notification-cleared") && data["id"] == question));
        }
        if action == "resolve" {
            service.plan_store.with_plan(&id, now, |plan| {
                plan.control_loop.next_at = None;
                Ok(())
            }).unwrap();
            assert_eq!(service.control_loop_show(&id).unwrap().next_at, Some(now + curator_delay(None) * 1000));
        }
    }
}

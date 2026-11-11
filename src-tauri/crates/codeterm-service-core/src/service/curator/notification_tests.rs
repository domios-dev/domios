use super::*;
use crate::notification::store::{Ask, NotificationProvenance, NotificationSeverity};
use crate::plan::CuratorWindow;
use codeterm_notification_core::contract::{NotificationAction, NotifyLevel};
use parking_lot::Mutex;
use std::sync::Arc;

#[test]
fn owner_chat_keeps_questions_open_until_explicit_resolution_resumes_curator() {
    for (action, choices) in [
        ("resolve", false),
        ("resolve", true),
        ("clear", true),
        ("tap", true),
    ] {
        let (fixture, id, _, owner) = crate::service::plan_authority::test_fixture::fixture();
        let service = &fixture.service;
        let owner = owner.tab_id.unwrap();
        let now = 1_700_000_000_000;
        let clock = crate::cron::clock::MockClock::starting_at(now);
        let cron = Arc::new(Mutex::new(
            crate::cron::store::CronStore::new_with_path_and_clock(
                fixture.temp.path().join("crons.json"),
                clock,
            ),
        ));
        assert!(service.set_ext(cron.clone()).is_ok());
        service.agent_registry.register_provider(Box::new(
            crate::agent::test_provider::MockProvider::new("provider-neutral"),
        ));
        service
            .mux
            .write()
            .get_tab_mut(&owner)
            .unwrap()
            .agent_session_model = Some("fixture-model".into());
        service
            .plan_store
            .edit_plan_fixture(&id, now, |plan| {
                plan.mode = PlanMode::Execution;
                Ok(())
            })
            .unwrap();
        let question = service.notif.lock().add_ext(
            "opaque question".into(),
            None,
            NotificationSeverity::Info,
            NotificationProvenance {
                tab_id: Some(owner.clone()),
                ..Default::default()
            },
            Ask::offering(choices.then(|| {
                vec![NotificationAction::Answer {
                    label: "opaque choice".into(),
                }]
            })),
            None,
            Some(NotifyLevel::ActionNeeded),
        );
        let parked = service.control_loop_show(&id).unwrap();
        assert_eq!(
            parked.suspended_reason,
            Some(CuratorSuspension::WaitingOnHuman)
        );
        assert_eq!(parked.next_at, None);
        assert!(service
            .resolve_notification(&question, Some("other-tab"))
            .is_err());
        for prepared in [false, true] {
            let input = crate::service::messenger::HumanInput::composer("unrelated owner message");
            let status = if prepared {
                let reservation = service.reserve_user_send(&owner, None).unwrap();
                service
                    .prepare_and_admit_user_send(reservation, input, false)
                    .unwrap()
                    .status
            } else {
                service
                    .start_human_chat_send(&owner, input, None, None)
                    .unwrap()
            };
            assert_eq!(
                service
                    .pending_delivery_payloads
                    .user_sends
                    .original(&owner, &status.send_id)
                    .unwrap(),
                "unrelated owner message"
            );
            let store = service.notif.lock();
            let record = store.notification(&question).unwrap();
            assert!(!record.resolved);
            assert_eq!(record.actions.is_some(), choices);
            assert_eq!(record.answer.is_some(), choices);
            if let Some(slot) = &record.answer {
                assert!(slot.answered.is_none());
                assert!(slot.answered_by.is_none());
                assert!(slot.answered_via.is_none());
                assert!(!slot.delivered);
            }
            drop(store);
            let waiting = service.control_loop_show(&id).unwrap();
            assert_eq!(
                waiting.suspended_reason,
                Some(CuratorSuspension::WaitingOnHuman)
            );
            assert_eq!(waiting.next_at, None);
            assert!(!fixture
                .ui
                .0
                .lock()
                .iter()
                .any(|(name, data)| { name == "notification-resolved" && data["id"] == question }));
        }
        match action {
            "resolve" => service
                .resolve_notification(&question, Some(&owner))
                .unwrap(),
            "clear" => service.clear_notifications(None),
            "tap" => {
                assert!(matches!(
                    service
                        .notif
                        .lock()
                        .record_answer(&question, "opaque choice"),
                    crate::notification::answer::AnswerAdmission::Recorded(_)
                ));
                service.reconcile_notification_questions();
            }
            _ => unreachable!(),
        }
        let state = service.plan_store.get(&id).unwrap().control_loop;
        assert_eq!(state.suspended_reason, None, "{action}");
        assert_eq!(
            state.next_at,
            Some(now + CuratorWindow::HOST.delay(None) * 1000),
            "{action}"
        );
        let job = cron
            .lock()
            .get(state.cron_id.as_ref().unwrap())
            .unwrap()
            .clone();
        assert!(job.enabled);
        assert_eq!(job.next_run, state.next_at);
        if action != "tap" {
            assert!(fixture
                .ui
                .0
                .lock()
                .iter()
                .any(|(name, data)| (name == "notification-resolved"
                    || name == "notification-cleared")
                    && data["id"] == question));
        }
        if action == "resolve" {
            service
                .plan_store
                .edit_plan_fixture(&id, now, |plan| {
                    plan.control_loop.next_at = None;
                    Ok(())
                })
                .unwrap();
            assert_eq!(
                service.control_loop_show(&id).unwrap().next_at,
                Some(now + CuratorWindow::HOST.delay(None) * 1000)
            );
        }
    }
}

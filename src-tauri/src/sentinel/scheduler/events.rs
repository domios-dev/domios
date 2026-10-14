use std::collections::BTreeSet;

use super::{tick, Handle};
use crate::service::ServiceContext;

impl Handle {
    pub fn notify_event_subscription_changed(&self, pipeline_id: &str) {
        let scope = self
            .store
            .lock()
            .get(pipeline_id)
            .map(|job| (job.workspace_id.clone(), job.team_id.clone()));
        if let (Some(app), Some((workspace_id, team_id))) = (&self.app, scope) {
            crate::api::events::emit_everywhere(
                app,
                "automation-graph-changed",
                serde_json::json!({
                    "workspace_id":workspace_id,
                    "team_id":team_id,
                    "revision":format!("event-subscription:{}", uuid::Uuid::new_v4().simple())
                }),
            );
        }
        self.notify();
    }
}

pub(super) fn reconcile_subscriptions(svc: &ServiceContext, handle: &Handle) {
    let jobs = handle.store.lock().list().to_vec();
    let now = handle.store.lock().clock.now_ms();
    let active = jobs
        .iter()
        .filter(|job| {
            job.pipeline().is_some_and(|pipeline| {
                matches!(
                    &pipeline.trigger,
                    codeterm_blueprint_types::blueprint_pipeline::Trigger::Event { .. }
                )
            }) && !job.machine.stopped
                && !job.machine.paused
                && !job.expires_at.is_some_and(|at| now >= at)
                && super::super::routes::owner_live(svc, job)
        })
        .map(|job| job.id.as_str())
        .collect::<BTreeSet<_>>();
    let registry = &svc.event_source_registry;
    let subscriptions = &svc.graph_event_subscriptions;
    let mut changed = BTreeSet::new();
    for binding in subscriptions.bindings() {
        let job = jobs.iter().find(|job| job.id == binding.pipeline_id);
        let selected = job.and_then(|job| match &job.pipeline()?.trigger {
            codeterm_blueprint_types::blueprint_pipeline::Trigger::Event { kind, params }
                if active.contains(job.id.as_str()) =>
            {
                registry
                    .select_provider(kind)
                    .map(|provider| (kind.clone(), params.clone(), provider))
            }
            _ => None,
        });
        let same = selected.as_ref().is_some_and(|(kind, params, provider)| {
            serde_json::to_string(params).ok().as_deref() == Some(binding.params_json.as_str())
                && kind == &binding.kind
                && provider.generation() == binding.registry_generation
                && job.is_some_and(|job| job.definition_version == binding.definition_version)
        });
        let owner_state = match job {
            None => codeterm_blueprint_types::blueprint_pipeline::SubscriptionOwnerState::Stopped,
            Some(job) if job.expires_at.is_some_and(|at| now >= at) => {
                codeterm_blueprint_types::blueprint_pipeline::SubscriptionOwnerState::Expired
            }
            Some(job) if job.machine.stopped || job.machine.paused => {
                codeterm_blueprint_types::blueprint_pipeline::SubscriptionOwnerState::Stopped
            }
            Some(job) if !super::super::routes::owner_live(svc, job) => {
                codeterm_blueprint_types::blueprint_pipeline::SubscriptionOwnerState::OwnerClosed
            }
            Some(_) if !same => {
                codeterm_blueprint_types::blueprint_pipeline::SubscriptionOwnerState::Amended
            }
            Some(_) => codeterm_blueprint_types::blueprint_pipeline::SubscriptionOwnerState::Active,
        };
        let provider_state = if selected.is_some() {
            codeterm_blueprint_types::blueprint_pipeline::SubscriptionProviderState::Available
        } else {
            codeterm_blueprint_types::blueprint_pipeline::SubscriptionProviderState::Missing
        };
        if codeterm_blueprint_types::blueprint_pipeline::next_subscription_action(
            true,
            owner_state,
            provider_state,
        ) == codeterm_blueprint_types::blueprint_pipeline::SubscriptionAction::Unsubscribe
            && subscriptions.unsubscribe(&binding.subscription_id)
        {
            changed.insert(binding.pipeline_id.clone());
        }
        if job.is_none() {
            subscriptions.clear_pipeline(&binding.pipeline_id);
            changed.insert(binding.pipeline_id.clone());
        }
    }
    let pipeline_ids = jobs
        .iter()
        .map(|job| job.id.as_str())
        .collect::<BTreeSet<_>>();
    for pipeline_id in subscriptions.pipeline_ids() {
        if !pipeline_ids.contains(pipeline_id.as_str()) {
            subscriptions.clear_pipeline(&pipeline_id);
        }
    }
    for job in jobs.iter().filter(|job| active.contains(job.id.as_str())) {
        let Some(codeterm_blueprint_types::blueprint_pipeline::Trigger::Event { kind, params }) =
            job.pipeline().map(|pipeline| &pipeline.trigger)
        else {
            continue;
        };
        if subscriptions.bindings_for(&job.id).iter().any(|binding| {
            registry.select_provider(kind).is_some_and(|provider| {
                binding.kind == *kind
                    && binding.params_json == serde_json::to_string(params).unwrap_or_default()
                    && binding.registry_generation == provider.generation()
                    && binding.definition_version == job.definition_version
            })
        }) {
            continue;
        }
        let Some(provider) = registry.select_provider(kind) else {
            if subscriptions
                .mark_unavailable_once(&job.id, "event source provider is missing or ungranted")
            {
                changed.insert(job.id.clone());
                super::super::routes::notify_trigger_unavailable(
                    svc,
                    job,
                    "event source provider is missing or ungranted",
                );
            }
            continue;
        };
        if let Some(schema) = provider.params_schema(kind) {
            if let Err(error) = schema.validate_instance(params) {
                let reason = format!("trigger.params: {error}");
                if subscriptions.mark_unavailable_once(&job.id, &reason) {
                    changed.insert(job.id.clone());
                    super::super::routes::notify_trigger_unavailable(svc, job, &reason);
                }
                continue;
            }
        }
        let subscription_id = uuid::Uuid::new_v4().simple().to_string();
        let token = uuid::Uuid::new_v4().simple().to_string();
        let params_json = serde_json::to_string(params).unwrap_or_else(|_| "{}".into());
        let inserted = subscriptions.subscribe(
            subscription_id.clone(),
            job.id.clone(),
            kind.clone(),
            params_json,
            provider.generation(),
            job.definition_version,
            provider.provider_handle(),
            token.clone(),
            provider.plugin_id().to_owned(),
            provider.payload_schema(kind),
        );
        let result = inserted.and_then(|()| {
            provider
                .provider()
                .subscribe(kind, params, &subscription_id, &token)
        });
        if let Err(error) = result {
            subscriptions.unsubscribe(&subscription_id);
            changed.insert(job.id.clone());
            if subscriptions.mark_unavailable_once(&job.id, &error) {
                super::super::routes::notify_trigger_unavailable(svc, job, &error);
            }
        } else {
            subscriptions.mark_subscribed(&job.id);
            changed.insert(job.id.clone());
        }
    }
    for pipeline_id in changed {
        handle.notify_event_subscription_changed(&pipeline_id);
    }
}

pub(super) fn dispatch_runs(svc: &crate::service::ServiceState, handle: &Handle) {
    let jobs = handle.store.lock().list().to_vec();
    for job in jobs.iter().filter(|job| {
        job.pipeline().is_some_and(|pipeline| {
            matches!(
                &pipeline.trigger,
                codeterm_blueprint_types::blueprint_pipeline::Trigger::Event { .. }
            )
        })
    }) {
        let Some(event) = svc.graph_event_subscriptions.next_for(&job.id) else {
            continue;
        };
        let Some(mut lease) = handle.store.lock().stage_event_lease(event.clone()) else {
            continue;
        };
        let Some(event) = svc
            .graph_event_subscriptions
            .consume(&event.subscription_id, &event.event_id)
        else {
            handle.store.lock().release(&job.id);
            continue;
        };
        lease.event = Some(event);
        let worker_handle = handle.clone();
        let worker_svc = svc.clone();
        std::thread::spawn(move || {
            tick(&worker_handle, &worker_svc, &lease);
            worker_handle.store.lock().release(&lease.job.id);
            worker_handle.notify();
        });
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use parking_lot::Mutex;
    use serde_json::json;

    use crate::sentinel::store::{Owner, Store};

    fn owner() -> Owner {
        Owner {
            creator_tab_id: "creator".into(),
            team_id: Some("team".into()),
            workspace_id: "workspace".into(),
            cwd: "/tmp".into(),
        }
    }

    #[test]
    fn blueprint_event_scheduler_stages_one_active_lease_without_a_poll_deadline() {
        let directory = tempfile::tempdir().unwrap();
        let clock = crate::cron::clock::MockClock::starting_at(1_000);
        let store = Arc::new(Mutex::new(Store::new(
            directory.path().join("events.json"),
            clock,
        )));
        let spec = codeterm_blueprint_types::blueprint_pipeline::BlueprintSpec {
            name: "event-lease-test".into(),
            owner: codeterm_blueprint_types::blueprint_pipeline::BlueprintOwner::Tab,
            trigger: codeterm_blueprint_types::blueprint_pipeline::Trigger::Event {
                kind: "fixture.message".into(),
                params: serde_json::json!({}),
            },
            steps: vec![codeterm_blueprint_types::blueprint_pipeline::Step {
                name: "match".into(),
                input: codeterm_blueprint_types::blueprint_pipeline::IoType::Text,
                kind: codeterm_blueprint_types::blueprint_pipeline::StepKind::Filter {
                    pattern: "fixture".into(),
                    bucket: None,
                },
                on: Default::default(),
            }],
            result: Default::default(),
        };
        let job = store.lock().prepare_pipeline(spec, owner(), false).unwrap();
        super::super::storage::commit(&store, |current| current.stage_insert(job)).unwrap();
        assert!(store.lock().event_wait_only());
        let event = codeterm_plugin::graph_events::EventEnvelope {
            subscription_id: "sub".into(),
            pipeline_id: store.lock().list()[0].id.clone(),
            kind: "fixture.message".into(),
            event_id: "event-1".into(),
            payload: json!({"message":"fixture"}),
            admitted_at_ms: 1_000,
        };
        let first = store.lock().stage_event_lease(event.clone()).unwrap();
        assert_eq!(first.event.as_ref().unwrap().payload, event.payload);
        assert!(store.lock().stage_event_lease(event).is_none());
        store.lock().release(&first.job.id);
        assert!(store.lock().event_wait_only());
    }
}

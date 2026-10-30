
use super::{canonical_dispatch_remedy, task_bound_mutating_spawn_ref};
use crate::plan::ExecutionIntent;
use crate::service::orchestrator::SpawnAgentParams;

#[test]
fn taskless_builtin_worker_with_plan_context_skips_task_bound_guard() {
    for kind in ["qa", "release_manager", "reviewer"] {
        for intent in [ExecutionIntent::Mutating, ExecutionIntent::ReadOnly] {
            let params = SpawnAgentParams {
                role: Some("Worker".into()),
                role_profile: Some(kind.into()),
                plan_id: Some("plan-bound-context".into()),
                execution_intent: Some(intent),
                cwd: Some("C:/repo".into()),
                ..Default::default()
            };

            assert_eq!(
                task_bound_mutating_spawn_ref(&params),
                None,
                "{kind} with Plan context stays on parent-directed task-less behavior"
            );
        }
    }

    let task_bound = SpawnAgentParams {
        task_id: Some("task-bound".into()),
        execution_intent: Some(ExecutionIntent::Mutating),
        ..Default::default()
    };
    assert_eq!(
        task_bound_mutating_spawn_ref(&task_bound),
        Some("task-bound")
    );
}

#[test]
fn canonical_dispatch_remedy_preserves_selected_builtin_worker_kind() {
    for kind in ["qa", "release_manager", "reviewer"] {
        assert_eq!(
                canonical_dispatch_remedy("plan", "task", Some(kind)),
                format!(
                    "Use parent-authorized canonical placement so its exclusive attempt is reserved before any tab exists: domios plan task dispatch plan task --execution-intent mutating --canonical-writer --kind {kind}"
                )
            );
    }
    assert_eq!(
            canonical_dispatch_remedy("plan", "task", Some("unavailable-overlay")),
            "Use parent-authorized canonical placement so its exclusive attempt is reserved before any tab exists: domios plan task dispatch plan task --execution-intent mutating --canonical-writer"
        );
}

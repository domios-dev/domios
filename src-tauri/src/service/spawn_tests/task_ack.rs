use super::*;
use std::collections::HashMap;

#[test]
fn task_bound_visible_spawn_keeps_the_assigned_task_starter() {
    let env = HashMap::from([
        ("CODETERM_PLAN_ID".into(), "plan-task".into()),
        ("CODETERM_TASK_ID".into(), "task-bound".into()),
        ("CODETERM_ATTEMPT_ID".into(), "attempt-task".into()),
    ]);
    let task_identity = crate::service::worker_ack::startup_task_identity_from_env(&env);
    assert!(task_identity.is_some());
    let selection = crate::service::launch_delivery::select_visible_spawn_task(
        None,
        Some("opaque-task-instructions"),
        true,
        true,
    );
    let startup = crate::service::launch_delivery::startup_starter_prompt(
        true,
        task_identity.is_some(),
    );

    assert_eq!(startup, None);
    assert_eq!(selection.confirmed_first_turn, None);
    assert_eq!(
        selection.provider_starter,
        Some(crate::agent::provider::StarterPrompt::AssignedTask)
    );
    assert_eq!(
        visible_spawn_starter_prompt(startup, selection.provider_starter),
        Some(crate::agent::provider::StarterPrompt::AssignedTask)
    );
}

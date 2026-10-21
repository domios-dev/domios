use super::*;

#[test]
fn worker_spawn_failure_reason_survives_persistence() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("workers.json");
    for (index, reason) in [
        "spawn_tab failed: provider capability refused",
        "spawn task failed: task panicked",
    ]
    .into_iter()
    .enumerate()
    {
        let registry = GraphWorkerRegistry::load(path.clone());
        let id = format!("failure-{index}");
        registry.register(make_info(
            id.clone(),
            String::new(),
            "extract:fixture".into(),
            "fixture".into(),
            String::new(),
        ));
        registry
            .finish_execution(
                &id,
                None,
                ExecutionEnd::ProcessEnded {
                    detail: reason.into(),
                },
            )
            .unwrap();
        let stored = GraphWorkerRegistry::load(path.clone()).get(&id).unwrap();
        assert_eq!(stored.status, WorkerStatus::Error);
        assert_eq!(stored.report.as_deref(), Some(reason));
        assert_eq!(stored.execution_end.unwrap().detail(), Some(reason));
        assert!(stored.done_at.is_some());
    }
}

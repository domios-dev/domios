pub(crate) fn notify(service: &crate::service::ServiceContext) {
    crate::service::launch_artifacts::sweep_after_restore(service);
}

#[cfg(test)]
mod tests {
    use super::notify;
    use crate::agent::provider::LaunchArtifactSweepRequest;
    use crate::agent::test_provider::MockProvider;
    use crate::multiplexer::state::MultiplexerState;
    use crate::service::test_fixture::ServiceFixture;
    use parking_lot::Mutex;
    use std::sync::Arc;

    #[test]
    #[cfg_attr(
        not(windows),
        ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
    )]
    fn startup_profile_sweep_runs_after_restore_with_the_live_tab_set() {
        let (mut mux, existing_tab_id) = MultiplexerState::new();
        let workspace_id = mux.list_workspaces()[0].id.clone();
        let restored_tab_id = mux.create_tab(&workspace_id).unwrap();
        mux.get_tab_mut(&existing_tab_id)
            .unwrap()
            .agent_type = Some("sweep-probe".into());
        let restored = mux.get_tab_mut(&restored_tab_id).unwrap();
        restored.agent_type = Some("sweep-probe".into());
        restored.restored_from_snapshot = true;

        let fixture = ServiceFixture::new(mux);
        let requests = Arc::new(Mutex::new(Vec::<LaunchArtifactSweepRequest>::new()));
        fixture.service.agent_registry.register_provider(Box::new(
            MockProvider::new("sweep-probe").with_launch_artifact_logs(
                requests.clone(),
                Arc::new(Mutex::new(Vec::new())),
            ),
        ));

        notify(&fixture.service);

        let requests = requests.lock();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].live_tab_ids.contains(&existing_tab_id));
        assert!(requests[0].live_tab_ids.contains(&restored_tab_id));
        assert!(requests[0].confirm);
        assert!(!requests[0].include_legacy);
    }
}

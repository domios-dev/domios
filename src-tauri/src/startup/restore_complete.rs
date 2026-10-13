/// The sweep walks provider plugin VMs and the disk, so it runs off the startup-critical path.
pub(crate) fn notify(service: &crate::service::ServiceState) -> std::thread::JoinHandle<()> {
    let service = service.clone();
    std::thread::Builder::new()
        .name("launch-artifact-sweep".into())
        .spawn(move || {
            let started = std::time::Instant::now();
            sweep(&service);
            log::info!(
                "[startup] launch artifact sweep took {:?}",
                started.elapsed()
            );
        })
        .expect("spawn launch artifact sweep thread")
}

fn sweep(service: &crate::service::ServiceContext) {
    crate::service::utility_deadline::restore(service);
    crate::service::launch_artifacts::sweep_after_restore(service);
}

#[cfg(test)]
mod tests {
    use super::sweep;
    use crate::agent::provider::LaunchArtifactSweepRequest;
    use crate::agent::test_provider::MockProvider;
    use crate::multiplexer::state::MultiplexerState;
    use crate::service::test_fixture::ServiceFixture;
    use parking_lot::Mutex;
    use std::sync::Arc;

    #[test]
    fn startup_profile_sweep_respects_machine_owner_and_runtime_dry_run_keeps_failed_tabs() {
        let (mut mux, existing_tab_id) = MultiplexerState::new();
        let workspace_id = mux.list_workspaces()[0].id.clone();
        let restored_tab_id = mux.create_tab(&workspace_id).unwrap();
        let failed_tab_id = mux.create_tab(&workspace_id).unwrap();
        let disproved_failure_tab_id = mux.create_tab(&workspace_id).unwrap();
        mux.get_tab_mut(&existing_tab_id).unwrap().agent_type = Some("sweep-probe".into());
        let restored = mux.get_tab_mut(&restored_tab_id).unwrap();
        restored.agent_type = Some("sweep-probe".into());
        restored.restored_from_snapshot = true;
        let failed_restore = mux.get_tab_mut(&failed_tab_id).unwrap();
        failed_restore.agent_type = Some("sweep-probe".into());
        failed_restore.pty_id = Some("pty-failed-restore-shell".into());
        failed_restore.boot_stage = Some(crate::service::boot_stage::BootStage::SpawnFailed);
        let disproved_failure = mux.get_tab_mut(&disproved_failure_tab_id).unwrap();
        disproved_failure.agent_type = Some("sweep-probe".into());
        disproved_failure.agent_session_id = Some("live-session".into());
        disproved_failure.session_binding_nonce = Some("live-nonce".into());
        disproved_failure.reported_session_binding_nonce = Some("live-nonce".into());
        disproved_failure.boot_stage = Some(crate::service::boot_stage::BootStage::SpawnFailed);
        assert!(crate::service::boot_stage::binding_disproves_latched_failure(disproved_failure));

        let fixture = ServiceFixture::new(mux);
        let requests = Arc::new(Mutex::new(Vec::<LaunchArtifactSweepRequest>::new()));
        fixture.service.agent_registry.register_provider(Box::new(
            MockProvider::new("sweep-probe")
                .with_launch_artifact_logs(requests.clone(), Arc::new(Mutex::new(Vec::new()))),
        ));

        sweep(&fixture.service);

        {
            let requests = requests.lock();
            let owner = codeterm_plugin::global_writes::machine_global_write_allowed();
            assert_eq!(requests.len(), usize::from(owner));
            if owner {
                assert!(requests[0].live_tab_ids.contains(&existing_tab_id));
                assert!(requests[0].live_tab_ids.contains(&restored_tab_id));
                assert!(requests[0].live_tab_ids.contains(&disproved_failure_tab_id));
                assert!(!requests[0].live_tab_ids.contains(&failed_tab_id));
                assert!(requests[0].confirm);
                assert!(requests[0].include_legacy);
            }
        }

        crate::service::launch_artifacts::sweep_for_owner(
            &fixture.service,
            "sweep-probe",
            false,
            false,
        )
        .expect("runtime sweep should succeed");

        let requests = requests.lock();
        let owner = codeterm_plugin::global_writes::machine_global_write_allowed();
        assert_eq!(requests.len(), 1 + usize::from(owner));
        let runtime = requests.last().unwrap();
        assert!(!runtime.confirm);
        assert!(runtime.live_tab_ids.contains(&failed_tab_id));
        assert!(runtime.live_tab_ids.contains(&disproved_failure_tab_id));
    }
}

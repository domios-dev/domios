use super::*;
use codeterm_plugin::provider::{ExecutionSandboxCapability, ExecutionSandboxMode};

#[test]
fn concierge_runtime_read_only_maps_only_its_own_pad_without_changing_cwd() {
    for (neighbor, intent, label, expected_mode) in [
        (
            true,
            ExecutionIntent::ReadOnly,
            "read-only",
            "opaque-pad-mode",
        ),
        (
            false,
            ExecutionIntent::ReadOnly,
            "read-only",
            "opaque-read-mode",
        ),
        (
            true,
            ExecutionIntent::Mutating,
            "workspace-write",
            "opaque-folder-mode",
        ),
    ] {
        let (mut mux, general) = crate::multiplexer::state::MultiplexerState::new();
        let workspace = mux.find_workspace_for_tab(&general).unwrap();
        let tab = mux.create_tab(&workspace).unwrap();
        if neighbor {
            mux.get_tab_mut(&tab).unwrap().neighbor_authority =
                Some(crate::multiplexer::types::NeighborAuthority {
                    uid: "owner".into(),
                    friendship_id: "a".repeat(64),
                    root_tab_id: None,
                });
        }
        let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
        fixture.service.agent_registry.register_provider(Box::new(
            crate::agent::test_provider::MockProvider::new("opaque-provider")
                .with_execution_sandbox(ExecutionSandboxCapability {
                    platforms: vec![crate::agent::provider::current_platform_tag().into()],
                    read_only: Some(ExecutionSandboxMode {
                        launch_args: vec!["opaque-read".into()],
                        runtime_mode: Some("opaque-read-mode".into()),
                    }),
                    mutating: Some(ExecutionSandboxMode {
                        launch_args: vec!["opaque-folder".into()],
                        runtime_mode: Some("opaque-folder-mode".into()),
                    }),
                    runtime_read_only_writable_roots_mode: Some("opaque-pad-mode".into()),
                    ..Default::default()
                }),
        ));
        let cwd = fixture.temp.path().join("friend");
        std::fs::create_dir(&cwd).unwrap();
        let params = SpawnAgentParams {
            agent: Some("opaque-provider".into()),
            cwd: Some(cwd.to_string_lossy().into_owned()),
            sandbox: Some(label.into()),
            ..Default::default()
        };
        let first =
            super::super::for_launch(&fixture.service, &tab, "opaque-provider", &params, true)
                .unwrap()
                .unwrap();
        assert_eq!(first.intent, intent);
        assert_eq!(first.runtime_mode.as_deref(), Some(expected_mode));
        assert_eq!(
            std::path::Path::new(&first.cwd).canonicalize().unwrap(),
            cwd.canonicalize().unwrap()
        );
        if neighbor {
            let pad = fixture
                .service
                .ensure_agent_pad(&tab)
                .unwrap()
                .path()
                .canonicalize()
                .unwrap();
            assert_eq!(first.writable_roots.len(), 1);
            assert_eq!(
                std::path::Path::new(&first.writable_roots[0])
                    .canonicalize()
                    .unwrap(),
                pad
            );
            assert!(!first.writable_roots.iter().any(|root| root == &first.cwd));
            assert!(first.environment.is_empty());
        } else {
            assert!(first.writable_roots.is_empty());
        }
        if neighbor {
            let foreign = fixture.temp.path().join("foreign-pad-or-cache");
            std::fs::create_dir(&foreign).unwrap();
            let foreign = foreign.canonicalize().unwrap();
            fixture
                .service
                .mux
                .write()
                .get_tab_mut(&tab)
                .unwrap()
                .restorable_launch
                .execution_sandbox
                .as_mut()
                .unwrap()
                .writable_roots
                .push(codeterm_paths::strip_verbatim_prefix(
                    foreign.to_str().unwrap(),
                ));
        }
        assert_eq!(
            super::super::for_launch(&fixture.service, &tab, "opaque-provider", &params, true)
                .unwrap(),
            Some(first)
        );
    }
}

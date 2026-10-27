use super::*;

fn caller(intent: ExecutionIntent, cwd: &Path) -> (String, ExecutionSandboxPolicy) {
    (
        "opaque-caller".into(),
        ExecutionSandboxPolicy {
            intent,
            cwd: codeterm_paths::strip_verbatim_prefix(
                cwd.canonicalize().unwrap().to_str().unwrap(),
            ),
            launch_args: Vec::new(),
            runtime_mode: None,
            writable_roots: Vec::new(),
            environment: Default::default(),
        },
    )
}

#[test]
fn unconfined_delegation_keeps_the_full_access_default_and_explicit_requests() {
    for requested in [None, Some("read-only"), Some("workspace-write")] {
        assert!(check(requested, None, &[], None).is_ok());
    }
}

#[test]
fn machine_ceiling_refuses_wider_requests_including_the_omitted_default() {
    for (ceiling, refused, admitted) in [
        (
            "read-only",
            vec![None, Some("workspace-write")],
            vec![Some("read-only")],
        ),
        (
            "workspace-write",
            vec![None],
            vec![Some("read-only"), Some("workspace-write")],
        ),
    ] {
        for requested in refused {
            let error = check(requested, Some(ceiling), &[], None).unwrap_err();
            assert!(error.contains("agent.sandbox_ceiling"), "{error}");
            assert!(error.contains(&format!("--sandbox {ceiling}")), "{error}");
        }
        for requested in admitted {
            assert!(check(requested, Some(ceiling), &[], None).is_ok());
        }
    }
    assert!(check(Some("read-only"), Some("opaque-ceiling"), &[], None)
        .unwrap_err()
        .contains("agent.sandbox_ceiling"));
}

#[test]
fn read_only_caller_admits_only_read_only_delegation() {
    let directory = tempfile::tempdir().unwrap();
    let callers = [caller(ExecutionIntent::ReadOnly, directory.path())];
    let cwd = directory.path().to_str();
    for requested in [None, Some("workspace-write")] {
        assert!(check(requested, None, &callers, cwd)
            .unwrap_err()
            .contains("opaque-caller"));
    }
    assert!(check(Some("read-only"), None, &callers, cwd).is_ok());
}

#[test]
fn workspace_write_caller_confines_delegated_writes_to_its_own_roots() {
    let lane = tempfile::tempdir().unwrap();
    let foreign = tempfile::tempdir().unwrap();
    let nested = lane.path().join("nested");
    std::fs::create_dir(&nested).unwrap();
    let callers = [caller(ExecutionIntent::Mutating, lane.path())];
    assert!(check(Some("workspace-write"), None, &callers, nested.to_str()).is_ok());
    assert!(check(Some("read-only"), None, &callers, foreign.path().to_str()).is_ok());
    for cwd in [foreign.path().to_str(), None] {
        assert!(check(Some("workspace-write"), None, &callers, cwd)
            .unwrap_err()
            .contains("writable roots"));
    }
    let escape = nested.join("..").join("..");
    assert!(check(Some("workspace-write"), None, &callers, escape.to_str()).is_err());
    assert!(check(None, None, &callers, nested.to_str()).is_err());
}

#[test]
fn service_bounds_named_and_header_callers_and_mesh_relay_by_recorded_policy() {
    let (mux, tab) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let named = Some(tab.as_str());
    let directory = tempfile::tempdir().unwrap();
    let unconfined = crate::service::SpawnAgentParams {
        spawned_by_tab_id: Some(tab.clone()),
        ..Default::default()
    };
    let enforce = super::super::enforce_delegation_ceiling;
    let mesh = super::super::refuse_confined_mesh_delegation;
    assert!(enforce(&fixture.service, &unconfined, named).is_ok());
    assert!(mesh(&fixture.service, &[named]).is_ok());
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab)
        .unwrap()
        .restorable_launch
        .execution_sandbox = Some(caller(ExecutionIntent::ReadOnly, directory.path()).1);
    for (params, header) in [
        (unconfined.clone(), None),
        (
            crate::service::SpawnAgentParams::default(),
            Some(tab.as_str()),
        ),
        (
            crate::service::SpawnAgentParams {
                parent_tab_id: Some(tab.clone()),
                ..Default::default()
            },
            None,
        ),
    ] {
        assert!(enforce(&fixture.service, &params, header)
            .unwrap_err()
            .contains(&tab));
    }
    let narrowed = crate::service::SpawnAgentParams {
        sandbox: Some("read-only".into()),
        ..unconfined
    };
    assert!(enforce(&fixture.service, &narrowed, named).is_ok());
    assert!(mesh(&fixture.service, &[None, named])
        .unwrap_err()
        .contains(&tab));
    assert!(enforce(&fixture.service, &Default::default(), None).is_ok());
    fixture
        .service
        .ext::<crate::config::ConfigState>()
        .lock()
        .agent
        .sandbox_ceiling = Some("workspace-write".into());
    assert!(enforce(&fixture.service, &Default::default(), None)
        .unwrap_err()
        .contains("agent.sandbox_ceiling"));
}

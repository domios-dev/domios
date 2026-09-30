use super::*;
use crate::agent::test_provider::MockProvider;
use crate::multiplexer::state::MultiplexerState;
use crate::service::orchestrator::SpawnAgentParams;
use crate::service::test_fixture::ServiceFixture;

#[test]
fn spawn_rejects_invalid_cwd() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("a-file.txt");
    std::fs::write(&file, b"not a directory").unwrap();
    let missing = dir.path().join("missing");

    let cases = [
        ("missing", missing.display().to_string()),
        ("file", file.display().to_string()),
        ("relative", "relative-dir-7f3a".to_string()),
    ];
    for (case, cwd) in cases {
        let error = validate_spawn_cwd(Some(cwd.as_str())).expect_err(case);
        assert!(error.contains(&cwd), "{case}: {error}");
        assert!(cwd_problem(&cwd).is_some(), "{case}");
    }

    assert_eq!(validate_spawn_cwd(dir.path().to_str()), Ok(()));
    assert_eq!(validate_spawn_cwd(None), Ok(()));
    assert_eq!(validate_spawn_cwd(Some("  ")), Ok(()));
}

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
fn spawn_rejects_invalid_cwd_before_creating_a_tab() {
    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        MockProvider::new("cwd-fixture").with_launch_binary("cmd.exe"),
    ));
    let file = fixture.temp.path().join("a-file.txt");
    std::fs::write(&file, b"not a directory").unwrap();
    let before = fixture.service.mux.read().snapshot().tabs.len();

    for cwd in [
        fixture.temp.path().join("missing").display().to_string(),
        file.display().to_string(),
        "relative-dir-7f3a".to_string(),
    ] {
        let error = fixture
            .service
            .spawn_agent_sync(SpawnAgentParams {
                agent: Some("cwd-fixture".into()),
                cwd: Some(cwd.clone()),
                ..Default::default()
            })
            .expect_err("an invalid cwd is refused");
        assert!(error.contains(&cwd), "{error}");
        assert_eq!(fixture.service.mux.read().snapshot().tabs.len(), before);
    }
}

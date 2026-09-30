use super::*;

const IDENTITY: &str = "opaque-restart-identity-7c2e";
const PROVIDER: &str = "claude";

struct RecordedSpawn {
    model: String,
    effort: String,
    model_flag: String,
    effort_args: Vec<String>,
}

fn recorded_spawn(svc: &ServiceContext) -> RecordedSpawn {
    svc.agent_registry
        .with_provider(PROVIDER, |provider| {
            let (model, effort) = provider
                .available_models()
                .into_iter()
                .find_map(|model| {
                    let effort = provider
                        .reasoning_efforts_for_model(Some(model.id.as_str()))
                        .into_iter()
                        .next()?;
                    Some((model.id, effort.id))
                })
                .expect("provider advertises a model with a reasoning effort");
            RecordedSpawn {
                model_flag: provider
                    .model_launch_args(&model)
                    .first()
                    .cloned()
                    .expect("provider renders a model flag"),
                effort_args: provider.reasoning_effort_launch_args(&effort),
                model,
                effort,
            }
        })
        .expect("provider registered")
}

fn restart_fixture(
    role: Option<&str>,
) -> (
    crate::service::test_fixture::ServiceFixture,
    TabRestartSnapshot,
    RecordedSpawn,
) {
    let fixture = crate::service::test_fixture::ServiceFixture::new(
        crate::multiplexer::state::MultiplexerState::new().0,
    );
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(crate::plugin::agent_claude::ClaudeProvider::new()));
    let spawn = recorded_spawn(&fixture.service);
    let tab_id = {
        let mut mux = fixture.service.mux.write();
        let workspace = mux.create_workspace("restart-args".into(), None).id;
        let tab_id = mux.create_tab(&workspace).unwrap();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some(PROVIDER.into());
        tab.agent_session_id = Some("session-restart".into());
        tab.restorable_launch = crate::multiplexer::types::RestorableLaunchParameters {
            captured: true,
            requested_model: Some(spawn.model.clone()),
            model: Some(spawn.model.clone()),
            requested_reasoning_effort: Some(spawn.effort.clone()),
            resolved_reasoning_effort: Some(spawn.effort.clone()),
            identity_instructions: Some(IDENTITY.into()),
            ..Default::default()
        };
        tab_id
    };
    let snap = TabRestartSnapshot {
        tab_id,
        pty_id: "pty-restart".into(),
        provider: PROVIDER.into(),
        session_id: Some("session-restart".into()),
        cwd: Some("fixture-cwd".into()),
        auto_approve: false,
        role: role.map(str::to_string),
        role_profile: None,
    };
    (fixture, snap, spawn)
}

fn delivered_prompt(command: &str) -> String {
    let path = command
        .rsplit_once("--append-system-prompt-file ")
        .expect("the relaunch names its system prompt file")
        .1
        .split_whitespace()
        .next()
        .unwrap()
        .trim_matches(['\'', '"']);
    let body = std::fs::read_to_string(path).expect("prompt file the command names");
    body.strip_prefix('\u{feff}').unwrap_or(&body).to_string()
}

fn assert_carries(command: &str, args: &[String]) {
    assert!(!args.is_empty());
    for arg in args {
        assert!(
            command.contains(arg.as_str()),
            "{arg} missing from {command}"
        );
    }
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn restart_preserves_launch_args() {
    for role in [None, Some("Worker")] {
        let (fixture, snap, spawn) = restart_fixture(role);

        let resumed = build_relaunch(&fixture.service, &snap, true).expect("resume relaunch");
        assert!(resumed.command.contains("session-restart"));
        assert!(resumed.command.contains(&spawn.model_flag));
        assert_carries(&resumed.command, &spawn.effort_args);
        assert_eq!(
            resumed
                .command
                .matches("--append-system-prompt-file")
                .count(),
            1
        );
        assert_eq!(delivered_prompt(&resumed.command), IDENTITY);
        assert_eq!(resumed.task.as_deref(), Some(IDENTITY));

        let fresh = build_relaunch(&fixture.service, &snap, false).expect("fresh relaunch");
        assert!(!fresh.command.contains("session-restart"));
        assert!(fresh.command.contains(&spawn.model_flag));
        assert_carries(&fresh.command, &spawn.effort_args);
        assert_eq!(delivered_prompt(&fresh.command), IDENTITY);
    }
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn restart_without_a_recorded_spawn_still_resumes_the_session() {
    let (fixture, snap, _) = restart_fixture(None);
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&snap.tab_id)
        .unwrap()
        .restorable_launch = Default::default();

    let resumed = build_relaunch(&fixture.service, &snap, true).expect("resume relaunch");

    assert!(resumed.command.contains("session-restart"));
    assert_eq!(resumed.task, None);
}

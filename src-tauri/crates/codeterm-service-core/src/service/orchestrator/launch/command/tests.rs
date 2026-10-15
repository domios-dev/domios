use super::*;
use crate::agent::provider::StructuredPromptServer;
use crate::agent::test_provider::MockProvider;
use parking_lot::Mutex;
use std::sync::Arc;

#[test]
fn structured_server_resume_preserves_the_session_in_launch_params() {
    let launches = Arc::new(Mutex::new(Vec::new()));
    let resumes = Arc::new(Mutex::new(Vec::new()));
    let registry = AgentRegistry::new();
    registry.register_provider(Box::new(
        MockProvider::new("server-fixture")
            .with_structured_prompt_launch_log(launches.clone())
            .with_resume_context_log(resumes.clone()),
    ));
    let params = SpawnAgentParams {
        resume_session_id: Some("owned-session".into()),
        structured_prompt_server: Some(StructuredPromptServer {
            hostname: "127.0.0.1".into(),
            port: 4567,
        }),
        args: Some(vec!["opaque-argument".into()]),
        ..Default::default()
    };

    let command = build_agent_command("server-fixture", &params, None, None, &registry);

    assert!(!command.is_empty());
    assert!(resumes.lock().is_empty());
    let launches = launches.lock();
    assert_eq!(launches.len(), 1);
    assert_eq!(launches[0].session_id.as_deref(), Some("owned-session"));
    let server = launches[0].structured_prompt_server.as_ref().unwrap();
    assert_eq!(server.hostname, "127.0.0.1");
    assert_eq!(server.port, 4567);
    assert_eq!(
        launches[0].args.as_deref(),
        Some(["opaque-argument".into()].as_slice())
    );
}

#[test]
fn plain_resume_keeps_the_provider_resume_contract() {
    let resumes = Arc::new(Mutex::new(Vec::new()));
    let registry = AgentRegistry::new();
    registry.register_provider(Box::new(
        MockProvider::new("plain-fixture").with_resume_context_log(resumes.clone()),
    ));
    let params = SpawnAgentParams {
        resume_session_id: Some("owned-session".into()),
        ..Default::default()
    };

    let command = build_agent_command("plain-fixture", &params, None, None, &registry);

    assert!(!command.is_empty());
    assert_eq!(resumes.lock().len(), 1);
    assert_eq!(resumes.lock()[0].session_id, "owned-session");
}

#[test]
fn restored_general_plain_resume_does_not_duplicate_the_typed_model_argument() {
    let resumes = Arc::new(Mutex::new(Vec::new()));
    let registry = AgentRegistry::new();
    registry.register_provider(Box::new(
        MockProvider::new("general-resume-fixture").with_resume_context_log(resumes.clone()),
    ));
    let params = SpawnAgentParams {
        resume_session_id: Some("owned-general-session".into()),
        model: Some("selected-model".into()),
        args: Some(vec!["--model".into(), "selected-model".into()]),
        role: Some("Manager".into()),
        role_profile: Some("general_agent".into()),
        ..Default::default()
    };
    let command = build_agent_command("general-resume-fixture", &params, None, None, &registry);
    assert!(!command.is_empty());
    assert_eq!(resumes.lock()[0].args, vec!["--model", "selected-model"]);
}

#[test]
fn structured_server_does_not_bypass_an_unsupported_session_fork() {
    let launches = Arc::new(Mutex::new(Vec::new()));
    let registry = AgentRegistry::new();
    registry.register_provider(Box::new(
        MockProvider::new("server-fork-fixture")
            .with_structured_prompt_launch_log(launches.clone()),
    ));
    let params = SpawnAgentParams {
        resume_session_id: Some("owned-session".into()),
        fork_session: true,
        structured_prompt_server: Some(StructuredPromptServer {
            hostname: "127.0.0.1".into(),
            port: 4567,
        }),
        ..Default::default()
    };

    assert!(build_agent_command("server-fork-fixture", &params, None, None, &registry).is_empty());
    assert!(launches.lock().is_empty());
}

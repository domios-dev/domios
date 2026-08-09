use super::*;

fn advertised() -> Vec<String> {
    ["alpha-agent", "beta-agent", "gamma-shell"]
        .iter()
        .map(|id| (*id).to_string())
        .collect()
}

/// The actual bug: an error that rejects a provider while listing it as
/// available. Asserted as a property of error construction over every
/// advertised id, so no future arm can reintroduce it.
#[test]
fn spawn_routing_rejection_never_advertises_the_provider_it_refuses() {
    let available = advertised();
    for requested in &available {
        let rejection = classify_unavailable_agent_provider(requested, available.clone());
        assert!(
            !rejection.advertises_requested_as_available(),
            "rejection for an advertised id must not list it as available: {rejection:?}"
        );
        assert_eq!(
            rejection.missing_capability(),
            Some(MissingSpawnCapability::PtyAgentRuntime),
            "an advertised id is refused for a named capability, never for being unknown"
        );
        for id in &available {
            assert!(
                !rejection.to_string().contains(&format!("[{id}")),
                "a capability rejection renders no availability list"
            );
        }
    }
}

/// Same invariant, asserted directly on the worker-identity rejection so both
/// constructors are covered rather than only the classifier.
#[test]
fn spawn_routing_worker_identity_rejection_never_advertises_the_provider_it_refuses() {
    let rejection = SpawnRejection::CapabilityUnavailable {
        requested: "gamma-shell".into(),
        missing: MissingSpawnCapability::WorkerIdentityHandoff,
    };
    assert!(!rejection.advertises_requested_as_available());
    assert_eq!(rejection.requested(), "gamma-shell");
    assert_eq!(
        rejection.missing_capability(),
        Some(MissingSpawnCapability::WorkerIdentityHandoff)
    );
}

/// A genuinely unregistered id still gets the availability list — that pairing
/// is evidence, not contradiction — and its rendering is unchanged.
#[test]
fn spawn_routing_unregistered_provider_keeps_the_available_list() {
    let available = advertised();
    let rejection = classify_unavailable_agent_provider("not-registered", available.clone());
    assert!(!rejection.advertises_requested_as_available());
    assert_eq!(rejection.missing_capability(), None);
    assert_eq!(
        rejection.to_string(),
        format!(
            "Unknown agent provider: '{}'. Available: [{}]",
            "not-registered",
            available.join(", ")
        )
    );
}

#[test]
fn spawn_routing_capability_ids_are_stable_discriminants() {
    assert_eq!(
        MissingSpawnCapability::PtyAgentRuntime.id(),
        "pty_agent_runtime"
    );
    assert_eq!(
        MissingSpawnCapability::WorkerIdentityHandoff.id(),
        "worker_identity_handoff"
    );
}

#[test]
fn spawn_routing_agent_launch_slot_exists_only_on_the_pty_command() {
    let mut pty = SpawnTabSpec {
        command: TabCommand::Agent(AgentSpawnSpec {
            provider: "alpha-agent".into(),
            ..Default::default()
        }),
        ..Default::default()
    };
    assert!(pty.bind_worker_identity("alpha-agent", "worker-1").is_ok());
    assert_eq!(
        pty.agent_launch_mut().and_then(|a| a.worker_id.clone()),
        Some("worker-1".to_string())
    );

    let mut chat = SpawnTabSpec {
        command: TabCommand::ChatBackend {
            plugin_id: "gamma-shell".into(),
            config: serde_json::Value::Null,
            role: None,
            role_profile: None,
            can_spawn: None,
            watcher: None,
        },
        ..Default::default()
    };
    assert!(chat.agent_launch_mut().is_none());
    let rejection = chat
        .bind_worker_identity("gamma-shell", "worker-1")
        .expect_err("a chat command has no worker-identity slot");
    assert_eq!(
        rejection.missing_capability(),
        Some(MissingSpawnCapability::WorkerIdentityHandoff)
    );
}

use super::*;

#[test]
fn product_app_started_has_only_version_platform_and_integer_counts() {
    assert_eq!(
        app_started("1.2.3-rc.1+private", "macos", 3, 4),
        Some(Event::AppStarted {
            version: Version::new(1, 2, 3),
            platform: Platform::Macos,
            mesh_machines: 3,
            workspaces: 4,
        })
    );
    assert!(app_started("private", "macos", 1, 1).is_none());
    assert!(app_started("1.2.3", "private", 1, 1).is_none());
}
#[test]
fn product_agent_spawned_has_only_closed_provider_and_role() {
    assert_eq!(
        agent_spawned(Some("codex"), Some("Worker")),
        Some(Event::AgentSpawned {
            provider_id: ProviderId::Codex,
            role: Role::Worker
        })
    );
    assert_eq!(
        agent_spawned(Some("private-provider"), Some("private-role")),
        Some(Event::AgentSpawned {
            provider_id: ProviderId::Other,
            role: Role::Other
        })
    );
    assert!(agent_spawned(None, None).is_none());
}
mod outcomes;

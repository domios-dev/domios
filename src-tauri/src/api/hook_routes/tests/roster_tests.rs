//! Hook event roster + codex Stop nonce-gate tests (split from tests.rs for the size gate).
use super::*;

struct RosterProvider {
    id: String,
    roster: Vec<HookEventBinding>,
}

impl AgentProvider for RosterProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn display_name(&self) -> &str {
        &self.id
    }

    fn build_launch_command(&self, _: &AgentLaunchParams) -> String {
        self.id.to_string()
    }

    fn readiness_strategy(&self) -> ReadinessStrategy {
        ReadinessStrategy::FixedDelay {
            duration: Duration::ZERO,
        }
    }

    fn task_delivery(&self) -> TaskDelivery {
        TaskDelivery::PtyInput
    }

    fn detect_from_title(&self, _: &str) -> bool {
        false
    }

    fn detect_from_output(&self, _: &str) -> bool {
        false
    }

    fn session_file_path(&self, _: &str, _: &str) -> Option<PathBuf> {
        None
    }

    fn detect_session_id(&self, _: &str, _: &[String], _: Option<Duration>) -> Option<String> {
        None
    }

    fn session_exists(&self, _: &str, _: &str) -> bool {
        true
    }

    fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
        None
    }

    fn has_hooks(&self) -> bool {
        true
    }

    fn hook_event_roster(&self) -> Vec<HookEventBinding> {
        self.roster.clone()
    }
}

fn binding(event: &str, route: HookIngestionRoute) -> HookEventBinding {
    HookEventBinding {
        event: event.into(),
        route,
        verdict: None,
    }
}

#[test]
fn hook_event_roster_registration_equals_declared_set() {
    let registry = AgentRegistry::new();
    registry.register_provider(Box::new(RosterProvider {
        id: "fixture-hooks".into(),
        roster: vec![
            binding("Notification", HookIngestionRoute::Permission),
            binding("SessionStart", HookIngestionRoute::SessionStart),
        ],
    }));
    registry.register_provider(Box::new(RosterProvider {
        id: "fixture-rosterless".into(),
        roster: vec![],
    }));

    let table = registered_hook_routes(&registry);
    let mut expected = std::collections::BTreeMap::new();
    expected.insert(
        "fixture-hooks".to_string(),
        vec![
            binding("Notification", HookIngestionRoute::Permission),
            binding("SessionStart", HookIngestionRoute::SessionStart),
        ],
    );
    assert_eq!(
        table, expected,
        "route table must equal exactly the declared set — no undeclared routes"
    );
}

#[test]
fn hook_event_roster_attributes_ingestion_to_non_claude_descriptor() {
    let (mux, tab_id) = shared_mux();
    mux.write().get_tab_mut(&tab_id).unwrap().cwd = Some("/tmp/project".into());
    let registry = AgentRegistry::new();
    registry.register_provider(Box::new(RosterProvider {
        id: "fixture-hooks".into(),
        roster: vec![
            binding("Notification", HookIngestionRoute::Permission),
            binding("SessionStart", HookIngestionRoute::SessionStart),
        ],
    }));

    // The permission route's owning descriptor is the fixture, never a literal.
    assert_eq!(
        sole_hook_route_declarer(&registry, HookIngestionRoute::Permission).as_deref(),
        Some("fixture-hooks")
    );

    // Round-trip: descriptor-resolved identity is what ingestion records.
    let provider_id = sole_hook_route_declarer(&registry, HookIngestionRoute::SessionStart)
        .expect("sole declarer resolves");
    bind_reported_session(&mux, &registry, &tab_id, &provider_id, "sid-a", None).unwrap();
    crate::agent::provider_identity::apply_provider_identity(
        mux.write().get_tab_mut(&tab_id).unwrap(),
        provider_id,
        true,
        crate::session::lifecycle::AgentTypeWriteOrigin::Hook,
    );
    assert_eq!(
        mux.read().get_tab(&tab_id).unwrap().agent_type.as_deref(),
        Some("fixture-hooks")
    );

    // Contested route → no guess.
    registry.register_provider(Box::new(RosterProvider {
        id: "fixture-rival".into(),
        roster: vec![binding("Notification", HookIngestionRoute::Permission)],
    }));
    assert_eq!(
        sole_hook_route_declarer(&registry, HookIngestionRoute::Permission),
        None
    );
}

#[test]
fn default_hook_event_roster_registration_matches_today() {
    use crate::agent::provider::session_start_hook_roster;
    use crate::plugin::agent_claude::ClaudeProvider;

    struct HooklessProvider;
    impl AgentProvider for HooklessProvider {
        fn id(&self) -> &str {
            "cursor-like"
        }
        fn display_name(&self) -> &str {
            "cursor-like"
        }
        fn build_launch_command(&self, _: &AgentLaunchParams) -> String {
            String::new()
        }
        fn readiness_strategy(&self) -> ReadinessStrategy {
            ReadinessStrategy::FixedDelay {
                duration: Duration::ZERO,
            }
        }
        fn task_delivery(&self) -> TaskDelivery {
            TaskDelivery::PtyInput
        }
        fn detect_from_title(&self, _: &str) -> bool {
            false
        }
        fn detect_from_output(&self, _: &str) -> bool {
            false
        }
        fn session_file_path(&self, _: &str, _: &str) -> Option<PathBuf> {
            None
        }
        fn detect_session_id(&self, _: &str, _: &[String], _: Option<Duration>) -> Option<String> {
            None
        }
        fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
            None
        }
    }

    let registry = AgentRegistry::new();
    registry.register_provider(Box::new(ClaudeProvider::new()));
    // Trait-default stand-in for today's hooked JS providers (codex/opencode).
    registry.register_provider(Box::new(HookProvider {
        id: "codex",
        sessions: HashSet::new(),
        materialization_root: None,
    }));
    registry.register_provider(Box::new(HooklessProvider));

    let table = registered_hook_routes(&registry);
    assert_eq!(
        table.keys().map(|id| id.as_str()).collect::<Vec<_>>(),
        ["claude", "codex"],
        "hookless providers register nothing"
    );
    assert_eq!(
        table["claude"]
            .iter()
            .map(|b| (b.event.as_str(), b.route))
            .collect::<Vec<_>>(),
        [
            ("PermissionRequest", HookIngestionRoute::Permission),
            ("Notification", HookIngestionRoute::Permission),
            ("Stop", HookIngestionRoute::Stop),
            ("PreCompact", HookIngestionRoute::MemFlush),
            ("SessionStart", HookIngestionRoute::SessionStart),
        ],
        "claude registers its current events"
    );
    assert_eq!(
        table["codex"],
        session_start_hook_roster(),
        "hooked providers without a declared roster register SessionStart only"
    );

    // Attribution identical to today: claude solely owns the permission route;
    // session_start is contested so identity falls through to the config default.
    assert_eq!(
        sole_hook_route_declarer(&registry, HookIngestionRoute::Permission).as_deref(),
        Some("claude")
    );
    assert_eq!(
        sole_hook_route_declarer(&registry, HookIngestionRoute::SessionStart),
        None
    );
}

#[test]
fn codex_stop_report_nonce_gate_verdicts() {
    // The completion-via-report gate: only an exact launch-nonce match is a
    // verified report; any carried nonce that can't be verified is foreign;
    assert_eq!(
        stop_report_verdict(Some("nonce-1"), Some("nonce-1")),
        StopReportVerdict::Verified
    );
    assert_eq!(
        stop_report_verdict(Some("nonce-1"), Some("nonce-2")),
        StopReportVerdict::Foreign
    );
    assert_eq!(
        stop_report_verdict(None, Some("nonce-1")),
        StopReportVerdict::Foreign
    );
    assert_eq!(
        stop_report_verdict(Some("nonce-1"), None),
        StopReportVerdict::Legacy
    );
    assert_eq!(stop_report_verdict(None, None), StopReportVerdict::Legacy);
}

#[test]
fn codex_stop_bundled_roster_declares_session_start_and_stop_and_registration_follows() {
    // Round-trip through the S1 mechanism: the shipped manifest's declared
    // roster (as data — provider id and bindings both read from the file)
    let manifest_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../plugins/provider-codex/plugin.json"
    );
    let manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(manifest_path).expect("bundled manifest"))
            .expect("manifest parses");
    let provider_id = manifest["id"].as_str().expect("manifest id").to_string();
    let roster: Vec<HookEventBinding> = manifest["spawn"]["hookEventRoster"]
        .as_array()
        .expect("declared hook event roster")
        .iter()
        .map(|entry| HookEventBinding {
            event: entry["event"].as_str().expect("event name").into(),
            route: HookIngestionRoute::parse(entry["route"].as_str().expect("route name"))
                .expect("route resolves against the closed ingestion set"),
            verdict: entry["verdict"].as_str().map(|name| {
                ProviderStateVerdict::parse(name)
                    .expect("verdict resolves against the closed verdict set")
            }),
        })
        .collect();
    assert_eq!(
        roster
            .iter()
            .map(|b| (b.event.as_str(), b.route))
            .collect::<Vec<_>>(),
        [
            ("SessionStart", HookIngestionRoute::SessionStart),
            ("Stop", HookIngestionRoute::Stop),
        ],
        "the declared roster is SessionStart + Stop, in declaration order"
    );

    let registry = AgentRegistry::new();
    registry.register_provider(Box::new(RosterProvider {
        id: provider_id.clone(),
        roster: roster.clone(),
    }));
    let table = registered_hook_routes(&registry);
    assert_eq!(
        table,
        BTreeMap::from([(provider_id, roster)]),
        "registration must equal exactly the declared roster"
    );
}

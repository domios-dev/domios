use super::*;
use codeterm_types::agent::capability_probe::*;
use serde_json::json;
fn identity(len: u64) -> model_catalog::BinaryFingerprint {
    model_catalog::BinaryFingerprint {
        path: "fixture".into(),
        len,
        modified_unix: 1,
    }
}
#[test]
fn claude_init_probe_exact_request_and_isolated_environment() {
    let request: serde_json::Value = serde_json::from_slice(protocol::REQUEST).unwrap();
    assert_eq!(
        request,
        json!({"type":"control_request","request_id":"probe-init","request":{"subtype":"initialize","hooks":null}})
    );
    assert_eq!(
        protocol::REQUEST
            .iter()
            .filter(|byte| **byte == b'\n')
            .count(),
        1
    );
    for key in [
        "CODETERM_TAB_ID",
        "codeterm_api_token",
        "FORCE_CODE_TERMINAL",
    ] {
        assert!(!protocol::inherited_key_allowed(key.as_ref()));
    }
    assert!(protocol::inherited_key_allowed("PATH".as_ref()));
    let args = protocol::args();
    assert_eq!(
        args,
        [
            "-p",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
            "--settings",
            "{\"disableAllHooks\":true}",
            "--strict-mcp-config",
            "--mcp-config",
            "{\"mcpServers\":{}}",
            "--no-session-persistence",
            "--allowedTools",
            ""
        ]
    );
}
#[test]
fn claude_init_probe_matches_control_response_and_decodes_permissively() {
    assert!(protocol::decode(&json!({"type":"assistant"})).is_none());
    assert!(protocol::decode(
        &json!({"type":"control_response","response":{"request_id":"other"}})
    )
    .is_none());
    let data=protocol::decode(&json!({"type":"control_response","response":{"request_id":"probe-init","subtype":"success","response":{
        "commands":[{"name":"fixture","argumentHint":"<opaque>","future":true},{}],"models":[{"value":"live-alias","displayName":"Live"}],
        "account":{"subscriptionType":"fixture-plan","apiProvider":"fixture"},"current_permission_mode":"plan","fast_mode_state":"on","unknown":{}}}})).unwrap().unwrap();
    assert_eq!(
        data.commands.unwrap()[0].argument_hint.as_deref(),
        Some("<opaque>")
    );
    assert_eq!(data.models.unwrap()[0].id, "live-alias");
    assert_eq!(
        data.account.unwrap().subscription_type.as_deref(),
        Some("fixture-plan")
    );
    assert_eq!(data.current_permission_mode.as_deref(), Some("plan"));
    let absent=protocol::decode(&json!({"type":"control_response","response":{"request_id":"probe-init","subtype":"success","response":{}}})).unwrap().unwrap();
    assert_eq!(absent, InitializeCapabilities::default());
    assert_eq!(
        protocol::decode(
            &json!({"type":"control_response","response":{"request_id":"probe-init","subtype":"error"}})
        ),
        Some(Err(ProbeFailure::Protocol))
    );
}
#[test]
fn claude_init_probe_cache_binary_project_login_and_refresh_invalidation() {
    let mut cache = ProbeCache::default();
    let key = ("fixture-provider".into(), PathBuf::from("project-a"));
    let Start::Run(first) = cache.begin(key.clone(), identity(1), "request-a", false) else {
        panic!()
    };
    assert!(matches!(
        cache.begin(key.clone(), identity(1), "request-b", false),
        Start::Cached(ProbeSnapshot::Pending)
    ));
    let ready = ProbeSnapshot::Ready {
        data: InitializeCapabilities::default(),
    };
    assert!(cache.finish(&key, first, ready.clone()));
    assert!(matches!(
        cache.begin(key.clone(), identity(1), "request-b", false),
        Start::Cached(ProbeSnapshot::Ready { .. })
    ));
    let Start::Run(binary) = cache.begin(key.clone(), identity(2), "request-b", false) else {
        panic!()
    };
    assert!(!cache.finish(&key, first, ready.clone()));
    let Start::Run(refresh) = cache.begin(key.clone(), identity(2), "request-c", true) else {
        panic!()
    };
    assert!(!cache.finish(&key, binary, ready.clone()));
    assert!(matches!(
        cache.begin(
            (key.0.clone(), PathBuf::from("project-b")),
            identity(2),
            "request-project-b",
            false
        ),
        Start::Run(_)
    ));
    assert_eq!(cache.invalidate(&key.0).len(), 2);
    assert!(!cache.finish(&key, refresh, ready));
    assert!(matches!(
        cache.begin(key, identity(2), "after-login", false),
        Start::Run(_)
    ));
}
#[test]
fn claude_init_probe_cache_bound_and_failure_are_stable() {
    let mut cache = ProbeCache::default();
    for index in 0..64 {
        assert!(matches!(
            cache.begin(
                ("fixture".into(), PathBuf::from(index.to_string())),
                identity(1),
                (),
                false
            ),
            Start::Run(_)
        ));
    }
    assert!(matches!(
        cache.begin(
            ("fixture".into(), PathBuf::from("overflow")),
            identity(1),
            (),
            false
        ),
        Start::Cached(ProbeSnapshot::Unavailable {
            reason: ProbeFailure::CacheLimit
        })
    ));
    let key = ("fixture".into(), PathBuf::from("0"));
    let Start::Run(token) = cache.begin(key.clone(), identity(1), (), true) else {
        panic!()
    };
    cache.finish(&key, token, unavailable(ProbeFailure::Deadline));
    assert!(matches!(
        cache.begin(key, identity(1), (), false),
        Start::Cached(ProbeSnapshot::Unavailable {
            reason: ProbeFailure::Deadline
        })
    ));
}
#[test]
fn claude_init_probe_model_merge_preserves_declared_effort_metadata() {
    let seed = ModelInfo {
        id: "alias".into(),
        display_name: "Seed".into(),
        default_reasoning_effort: Some("high".into()),
        ..Default::default()
    };
    let models = merge_model_entries(
        vec![seed],
        &[
            ProbeModel {
                id: "alias".into(),
                display_name: Some("Live".into()),
                description: None,
            },
            ProbeModel {
                id: "new-alias".into(),
                display_name: None,
                description: None,
            },
        ],
    );
    assert_eq!(models.len(), 2);
    assert_eq!(models[0].display_name, "Live");
    assert_eq!(models[0].default_reasoning_effort.as_deref(), Some("high"));
    assert_eq!(models[1].id, "new-alias");
}

struct HostFixture {
    enabled: std::sync::atomic::AtomicBool,
    published: Mutex<Vec<ProbeSnapshot>>,
}

impl CapabilityProbeHost for HostFixture {
    fn validate(&self, _: &str) -> Result<ProbeTarget, ProbeFailure> {
        if !self.enabled.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(ProbeFailure::Disabled);
        }
        Ok(ProbeTarget {
            provider: "fixture".into(),
            project: "project".into(),
        })
    }

    fn cli_binary(&self, _: &str) -> Option<String> {
        panic!("validation failure must stop before binary lookup")
    }

    fn matching_tabs(&self, _: &str, _: Option<&Path>) -> Vec<String> {
        Vec::new()
    }

    fn publish(&self, provider: &str, project: Option<&Path>, snapshot: Option<&ProbeSnapshot>) {
        assert_eq!(provider, "fixture");
        assert_eq!(project, Some(Path::new("project")));
        self.published.lock().push(snapshot.unwrap().clone());
    }
}

#[test]
fn rejected_host_request_does_not_resolve_or_publish() {
    let host = Arc::new(HostFixture {
        enabled: false.into(),
        published: Mutex::default(),
    });
    assert_eq!(
        refresh(host.clone(), "tab", false),
        unavailable(ProbeFailure::Disabled)
    );
    assert!(host.published.lock().is_empty());
}

#[test]
fn publication_revalidates_the_originating_tab() {
    let host = Arc::new(HostFixture {
        enabled: true.into(),
        published: Mutex::default(),
    });
    let request = request(host.clone(), "tab").unwrap();
    publish(&request, &ProbeSnapshot::Pending);
    host.enabled
        .store(false, std::sync::atomic::Ordering::Relaxed);
    publish(&request, &unavailable(ProbeFailure::Protocol));
    assert_eq!(*host.published.lock(), vec![ProbeSnapshot::Pending]);
}

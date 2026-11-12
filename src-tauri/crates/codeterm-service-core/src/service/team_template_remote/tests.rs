use std::collections::BTreeMap;
use std::sync::Arc;

use codeterm_team_package::contracts::{TemplateRef, TemplateStartRefusal, TemplateStartRequest};
use parking_lot::Mutex;
use serde_json::json;

use super::*;
use crate::service::team_template_start::resolve;

#[test]
fn hosting_gate_requires_the_feature_at_a_supported_version() {
    let current =
        json!({"version": TEMPLATE_HOSTING_MIN_VERSION, "features": [TEMPLATE_HOSTING_FEATURE]});
    assert!(hosting_gate(&current, "mac").is_ok());
    let prerelease = json!({"version": format!("{TEMPLATE_HOSTING_MIN_VERSION}-dev"), "features": [TEMPLATE_HOSTING_FEATURE]});
    assert!(hosting_gate(&prerelease, "mac").is_ok());

    let flagless = json!({"version": "1.19.2", "features": ["remote_task_dispatch"]});
    let refusal = hosting_gate(&flagless, "mac").unwrap_err();
    assert!(refusal.starts_with("peer_version_unsupported"));
    assert!(refusal.contains("1.19.2") && refusal.contains(TEMPLATE_HOSTING_MIN_VERSION));

    let old = json!({"version": "1.18.0", "features": [TEMPLATE_HOSTING_FEATURE]});
    assert!(hosting_gate(&old, "mac").is_err());
    assert!(hosting_gate(&json!({}), "mac")
        .unwrap_err()
        .contains("unknown"));
}

#[test]
fn hosted_brief_keeps_the_startup_ack_first() {
    let role = HostedRole {
        kind: "kind-a".into(),
        guidance: "GUIDANCE-FRAGMENT".into(),
    };
    let brief = compose_hosted_brief("ACK-FRAGMENT\n\nBODY-FRAGMENT", &role);
    let ack = brief.find("ACK-FRAGMENT").unwrap();
    let guidance = brief.find("GUIDANCE-FRAGMENT").unwrap();
    let body = brief.find("BODY-FRAGMENT").unwrap();
    assert!(ack < guidance && guidance < body);
    let empty = HostedRole {
        kind: "kind-a".into(),
        guidance: " ".into(),
    };
    assert_eq!(compose_hosted_brief("ACK\n\nBODY", &empty), "ACK\n\nBODY");
}

#[derive(Default)]
struct FakeAwake {
    already_active: bool,
    calls: Mutex<Vec<&'static str>>,
}

impl AwakePort for FakeAwake {
    fn hold(&self) -> Result<bool, String> {
        self.calls.lock().push("hold");
        Ok(!self.already_active)
    }

    fn release(&self) -> Result<(), String> {
        self.calls.lock().push("release");
        Ok(())
    }
}

#[test]
fn the_first_hosted_role_holds_awake_and_the_last_release_stops_it() {
    let leases = HostingLeases::default();
    let port = Arc::new(FakeAwake::default());
    leases.acquire("attempt-1", port.clone()).unwrap();
    leases.acquire("attempt-1", port.clone()).unwrap();
    leases.transfer("attempt-1", "tab-1");
    leases.acquire("tab-2", port.clone()).unwrap();
    assert!(leases.holds("tab-1") && !leases.holds("attempt-1"));
    leases.release("tab-1").unwrap();
    assert_eq!(*port.calls.lock(), vec!["hold"]);
    leases.release("tab-2").unwrap();
    leases.release("tab-2").unwrap();
    assert_eq!(*port.calls.lock(), vec!["hold", "release"]);
}

#[test]
fn a_failed_spawn_releases_its_lease() {
    let leases = HostingLeases::default();
    let port = Arc::new(FakeAwake::default());
    leases.acquire("attempt-failed", port.clone()).unwrap();
    leases.release("attempt-failed").unwrap();
    assert!(!leases.holds("attempt-failed"));
    assert_eq!(*port.calls.lock(), vec!["hold", "release"]);
}

#[test]
fn an_owner_keepawake_is_borrowed_and_never_stopped() {
    let leases = HostingLeases::default();
    let port = Arc::new(FakeAwake {
        already_active: true,
        ..Default::default()
    });
    leases.acquire("tab-1", port.clone()).unwrap();
    leases.release("tab-1").unwrap();
    assert_eq!(*port.calls.lock(), vec!["hold"]);
}

fn request(name: &str) -> TemplateStartRequest {
    TemplateStartRequest {
        template: TemplateRef {
            name: name.into(),
            version: None,
        },
        team_id: None,
        inputs: BTreeMap::new(),
        opt_out: Vec::new(),
        decision_id: None,
        secret_bindings: BTreeMap::new(),
        manager_models: BTreeMap::new(),
    }
}

#[test]
fn vault_bindings_are_checked_by_entry_name_only() {
    let mut start = request("default");
    start
        .secret_bindings
        .insert("api".into(), "peer-entry-present".into());
    start
        .secret_bindings
        .insert("token".into(), "peer-entry-absent".into());
    let mut asked = Vec::new();
    let unbound = unbound_secrets(&start, |entry| {
        asked.push(entry.to_string());
        Ok(entry == "peer-entry-present")
    });
    assert_eq!(unbound, vec!["secret:token".to_string()]);
    assert_eq!(asked, vec!["peer-entry-present", "peer-entry-absent"]);
    assert_eq!(
        unbound_secrets(&start, |_| Err("vault locked".into())).len(),
        2
    );
}

#[test]
fn a_mesh_start_needs_full_access_and_a_fresh_team() {
    let ledger = ConsentLedger::default();
    let spent = SpentDecisions::default();
    assert_eq!(
        admit_from_peer(&request("default"), false, &ledger, &spent, Vec::new()).unwrap_err(),
        PeerStartRefusal::Forbidden
    );
    let mut rerun = request("default");
    rerun.team_id = Some("team-origin".into());
    assert_eq!(
        admit_from_peer(&rerun, true, &ledger, &spent, Vec::new()).unwrap_err(),
        PeerStartRefusal::OriginTeam
    );
}

#[test]
fn a_mesh_start_needs_one_local_owner_decision_even_without_asks() {
    let ledger = ConsentLedger::default();
    let spent = SpentDecisions::default();
    let mut start = request("default");
    assert!(matches!(
        admit_from_peer(&start, true, &ledger, &spent, Vec::new()),
        Err(PeerStartRefusal::Refused(
            TemplateStartRefusal::ConsentRequired { .. }
        ))
    ));
    start.decision_id = Some("consent_forged".into());
    assert!(admit_from_peer(&start, true, &ledger, &spent, Vec::new()).is_err());

    let identity = resolve(None).unwrap().identity();
    start.decision_id = Some(ledger.issue(&identity, &[]));
    assert!(matches!(
        admit_from_peer(&start, true, &ledger, &spent, vec!["secret:token".into()]),
        Err(PeerStartRefusal::Refused(
            TemplateStartRefusal::MissingInputs { .. }
        ))
    ));
    assert!(admit_from_peer(&start, true, &ledger, &spent, Vec::new()).is_ok());
    assert_eq!(
        admit_from_peer(&start, true, &ledger, &spent, Vec::new()).unwrap_err(),
        PeerStartRefusal::DecisionSpent
    );
}

#[test]
fn the_owner_tap_records_the_decision_a_mesh_start_needs() {
    let ledger = ConsentLedger::default();
    let spent = SpentDecisions::default();
    let start = request("default");
    assert!(precheck_peer_start(&start, true, Vec::new()).is_ok());
    assert!(matches!(
        precheck_peer_start(&start, true, vec!["secret:token".into()]),
        Err(PeerStartRefusal::Refused(
            TemplateStartRefusal::MissingInputs { .. }
        ))
    ));
    assert_eq!(
        precheck_peer_start(&start, false, Vec::new()).unwrap_err(),
        PeerStartRefusal::Forbidden
    );
    let decided = record_owner_decision(&start, &ledger).unwrap();
    assert!(admit_from_peer(&decided, true, &ledger, &spent, Vec::new()).is_ok());
}

#[test]
fn a_peer_ask_runs_at_most_once() {
    let asks = PeerAsks::default();
    let request = asks.open(PeerAsk::Clone {
        origin_peer: "origin".into(),
        url: "https://example.test/o/r.git".into(),
        path: std::path::PathBuf::from("r"),
    });
    assert!(request.starts_with(PEER_ASK_PREFIX));
    assert_eq!(
        asks.open_clone("https://example.test/o/r.git").as_deref(),
        Some(request.as_str())
    );
    assert_eq!(asks.outcome(&request).unwrap()["state"], "awaiting_owner");
    assert!(asks.take(&request).is_some());
    assert!(asks.take(&request).is_none());
    assert!(asks.outcome(&request).is_none());
    asks.finish(&request, json!({"state": "declined"}));
    assert_eq!(asks.outcome(&request).unwrap()["state"], "declined");
}

#[test]
fn clone_urls_never_carry_credentials() {
    assert_eq!(
        scrub_remote_url("https://x-access-token:SECRET-FRAGMENT@github.com/o/r.git?x=1#f")
            .as_deref(),
        Some("https://github.com/o/r.git")
    );
    assert_eq!(
        scrub_remote_url("ssh://git:SECRET-FRAGMENT@host.test:22/o/r.git").as_deref(),
        Some("ssh://git@host.test:22/o/r.git")
    );
    assert_eq!(
        scrub_remote_url("git@github.com:o/r.git").as_deref(),
        Some("git@github.com:o/r.git")
    );
    for refused in [
        "",
        "/home/me/repo",
        "file:///home/me/repo",
        "user:SECRET-FRAGMENT@host:o/r",
        "https://github.com/",
    ] {
        assert_eq!(scrub_remote_url(refused), None, "{refused}");
    }
}

#[test]
fn a_clone_lands_under_the_root_by_repository_name() {
    let root = std::path::Path::new("root");
    assert_eq!(
        clone_destination(root, "https://github.com/o/repo.git"),
        Some(root.join("repo"))
    );
    assert_eq!(
        clone_destination(root, "git@github.com:o/repo"),
        Some(root.join("repo"))
    );
    assert_eq!(clone_destination(root, "https://github.com/o/.."), None);
}

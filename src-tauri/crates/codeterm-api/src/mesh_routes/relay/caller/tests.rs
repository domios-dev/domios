// WHAT: local-origin mesh actor attribution before peer resolution.
// WHY: a signed relay must not launder caller-supplied local identity.
// ORACLE: own actors derive; foreign actors refuse; other payloads stay intact.
use super::*;
use serde_json::json;

fn caller() -> TabCaller {
    TabCaller { tab_id: "own-tab".into(), binding_nonce: "own-launch".into() }
}

#[test]
fn derives_local_spawn_and_send_actors() {
    let mut spawn = json!({"parent_tab_id":"target-parent"});
    bind(&caller(), "POST", "/api/agents/spawn?local=1", &mut spawn).unwrap();
    assert_eq!(spawn["spawned_by_tab_id"], "own-tab");
    assert_eq!(spawn["creator_tab_id"], "own-tab");
    assert_eq!(spawn["parent_tab_id"], "target-parent");
    let mut send = json!({"text":"opaque-payload"});
    bind(&caller(), "post", "/api/tabs/remote-target/send", &mut send).unwrap();
    assert_eq!(send["source_tab"], "own-tab");
    assert_eq!(send["text"], "opaque-payload");
}

#[test]
fn refuses_cross_tab_and_peer_provenance_before_forwarding() {
    for field in ["spawned_by_tab_id", "creator_tab_id", "origin_peer"] {
        let mut body = json!({});
        body[field] = json!("foreign");
        assert!(bind(&caller(), "POST", "/api/agents/spawn", &mut body).is_err(), "{field}");
    }
    for field in ["source_tab", "source_peer", "system_context"] {
        let mut body = json!({});
        body[field] = json!("foreign");
        assert!(bind(&caller(), "POST", "/api/tabs/target/send", &mut body).is_err(), "{field}");
    }
}

#[test]
fn leaves_unrelated_relay_payloads_unchanged() {
    let original = json!({"source_tab":"foreign"});
    for (method, path) in [("GET", "/api/tabs/target/send"), ("POST", "/api/config")] {
        let mut body = original.clone();
        bind(&caller(), method, path, &mut body).unwrap();
        assert_eq!(body, original);
    }
}

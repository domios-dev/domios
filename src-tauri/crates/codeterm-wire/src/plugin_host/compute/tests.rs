use super::*;

#[test]
fn host_responses_omit_unreported_fields() {
    assert_eq!(
        serde_json::json!(HostWorkerPoll {
            done: false,
            report: None,
            error: None
        }),
        serde_json::json!({"done":false})
    );
    assert_eq!(
        serde_json::json!(HostAgentPoll {
            done: true,
            reply: None,
            error: Some("missing".into())
        }),
        serde_json::json!({"done":true,"error":"missing"})
    );
    assert_eq!(
        serde_json::json!(HostRefusal {
            error: HostError {
                error: "denied".into()
            },
            message: None
        }),
        serde_json::json!({"error":"denied"})
    );
    assert_eq!(
        serde_json::json!(CoreMachineEngine::Machine {
            charter: "opaque".into(),
            tick_interval_secs: None
        }),
        serde_json::json!({"charter":"opaque","kind":"machine"})
    );
}

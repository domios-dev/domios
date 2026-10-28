use super::*;
use codeterm_account::account_mesh::callable::CallableFailure;
use parking_lot::Mutex;
use std::collections::VecDeque;

struct Transport {
    calls: Mutex<Vec<(String, Value)>>,
    replies: Mutex<VecDeque<Value>>,
}
#[async_trait::async_trait]
impl CallableTransport for Transport {
    async fn call(&self, route: &str, token: &str, data: Value) -> Result<Value, CallableFailure> {
        assert_eq!(token, "fixture-token");
        self.calls.lock().push((route.into(), data));
        Ok(json!({"result":self.replies.lock().pop_front().unwrap()}))
    }
}
fn transport(replies: Vec<Value>) -> Transport {
    Transport {
        calls: Mutex::new(vec![]),
        replies: Mutex::new(replies.into()),
    }
}
#[tokio::test]
async fn explicit_opt_in_reads_revision_then_links_exact_local_device_without_properties() {
    let device = uuid::Uuid::new_v4();
    let port = transport(vec![
        json!({"status":"current","revision":4}),
        json!({"status":"applied"}),
        json!({"status":"recorded"}),
    ]);
    synchronize(
        &port,
        "fixture-token",
        Consent::On,
        Some(device),
        Some(Consent::On),
    )
    .await
    .unwrap();
    let calls = port.calls.lock();
    assert_eq!(
        calls[0],
        ("analyticsConsent".into(), json!({"action":"read"}))
    );
    assert_eq!(calls[1].1["base_revision"], 4);
    assert_eq!(calls[1].1["state"], "on");
    assert_eq!(calls[2].0, "analyticsSignIn");
    assert_eq!(calls[2].1["device_id"], device.to_string());
    assert_eq!(calls[2].1.as_object().unwrap().len(), 2);
}
#[tokio::test]
async fn withdrawal_sends_only_off_and_restore_never_overrides_server_off() {
    let port = transport(vec![json!({"status":"applied"})]);
    synchronize(
        &port,
        "fixture-token",
        Consent::Off,
        None,
        Some(Consent::Off),
    )
    .await
    .unwrap();
    assert_eq!(port.calls.lock().len(), 1);
    assert_eq!(port.calls.lock()[0].1["state"], "off");
    let port = transport(vec![json!({"status":"off"})]);
    assert!(synchronize(
        &port,
        "fixture-token",
        Consent::On,
        Some(uuid::Uuid::new_v4()),
        None
    )
    .await
    .is_err());
    assert_eq!(port.calls.lock()[0].0, "analyticsSignIn");
    assert_eq!(port.calls.lock().len(), 1);
}
#[tokio::test]
async fn local_off_is_silent_and_stale_consent_does_not_link() {
    let port = transport(vec![]);
    synchronize(&port, "fixture-token", Consent::Off, None, None)
        .await
        .unwrap();
    assert!(port.calls.lock().is_empty());
    let port = transport(vec![json!({"revision":0}), json!({"status":"stale"})]);
    assert!(synchronize(
        &port,
        "fixture-token",
        Consent::On,
        Some(uuid::Uuid::new_v4()),
        Some(Consent::On)
    )
    .await
    .is_err());
    assert_eq!(port.calls.lock().len(), 2);
}

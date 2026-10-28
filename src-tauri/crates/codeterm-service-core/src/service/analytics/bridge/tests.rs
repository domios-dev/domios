use super::*;
use codeterm_account::account_mesh::callable::CallableFailure;
use codeterm_analytics::{
    sinks::fake::{Fake, Recording},
    Config, Event, Mode,
};
use parking_lot::Mutex;
use std::{collections::VecDeque, sync::Arc};
use tokio::sync::oneshot;

struct Transport {
    calls: Mutex<Vec<(String, Value)>>,
    replies: Mutex<VecDeque<Result<Value, CallableFailure>>>,
    entered: Mutex<Option<oneshot::Sender<()>>>,
    release: Mutex<Option<oneshot::Receiver<()>>>,
}
#[async_trait::async_trait]
impl CallableTransport for Transport {
    async fn call(&self, route: &str, token: &str, data: Value) -> Result<Value, CallableFailure> {
        assert_eq!(token, "fixture-token");
        self.calls.lock().push((route.into(), data));
        let entered = self.entered.lock().take();
        let release = self.release.lock().take();
        if let Some(entered) = entered {
            entered.send(()).unwrap();
        }
        if let Some(release) = release {
            release.await.unwrap();
        }
        self.replies
            .lock()
            .pop_front()
            .expect("unexpected RPC")
            .map(|reply| json!({"result":reply}))
    }
}
fn transport(replies: Vec<Value>) -> Transport {
    Transport {
        calls: Mutex::new(vec![]),
        replies: Mutex::new(replies.into_iter().map(Ok).collect()),
        entered: Mutex::new(None),
        release: Mutex::new(None),
    }
}
fn worker(path: std::path::PathBuf, recording: Arc<std::sync::Mutex<Recording>>) -> Worker {
    Worker::start_with_sink(
        path,
        Config {
            mode: Mode::Amplitude,
            ..Config::default()
        },
        Box::new(Fake { recording }),
    )
    .unwrap()
}
struct Fixture {
    _dir: tempfile::TempDir,
    worker: Worker,
    recording: Arc<std::sync::Mutex<Recording>>,
    bridge: Bridge,
}
impl Fixture {
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let recording = Arc::new(std::sync::Mutex::new(Recording::default()));
        let worker = worker(dir.path().join("settings.json"), recording.clone());
        worker
            .account_changed(Some("account-X".into()))
            .await
            .unwrap();
        Self {
            _dir: dir,
            worker,
            recording,
            bridge: Bridge::default(),
        }
    }
    async fn sync(&self, port: &Transport, intent: Option<Consent>) -> Result<(), String> {
        let ticket = self.worker.begin_consent(intent).await?;
        synchronize(
            &self.bridge,
            &self.worker,
            port,
            "fixture-token",
            "account-X",
            ticket,
            intent,
        )
        .await
    }
    async fn no_delivery(&self) {
        assert!(!self.worker.record(Event::PlanViewed {}).await.unwrap());
        self.worker.flush().await.unwrap();
        let status = self.worker.status().await.unwrap();
        assert!(!status.has_user_id);
        assert_eq!(status.queued, 0);
        assert!(self.recording.lock().unwrap().batches.is_empty());
    }
}

#[tokio::test]
async fn explicit_opt_in_reads_revision_then_links_exact_local_device_without_properties() {
    let f = Fixture::new().await;
    let port = transport(vec![
        json!({"revision":4}),
        json!({"status":"applied"}),
        json!({"status":"recorded"}),
    ]);
    f.sync(&port, Some(Consent::On)).await.unwrap();
    assert!(f.worker.status().await.unwrap().has_user_id);
    let device = f.worker.device_id().await.unwrap().unwrap();
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
    let f = Fixture::new().await;
    let port = transport(vec![json!({"status":"applied"})]);
    f.sync(&port, Some(Consent::Off)).await.unwrap();
    assert_eq!(port.calls.lock().len(), 1);
    assert_eq!(port.calls.lock()[0].1["state"], "off");
    f.no_delivery().await;
    let port = transport(vec![]);
    f.sync(&port, None).await.unwrap();
    assert!(port.calls.lock().is_empty());
}

#[tokio::test]
async fn local_off_is_silent_and_stale_consent_does_not_link() {
    let f = Fixture::new().await;
    let port = transport(vec![json!({"revision":0}), json!({"status":"stale"})]);
    assert!(f.sync(&port, Some(Consent::On)).await.is_err());
    assert_eq!(port.calls.lock().len(), 2);
    f.no_delivery().await;
}

#[tokio::test]
async fn server_off_erasure_and_consent_refusals_deliver_zero_events() {
    for response in [
        json!({"status":"off"}),
        json!({"status":"refused","reason":"erased"}),
    ] {
        for stage in 0..3 {
            let f = Fixture::new().await;
            let approved = f.worker.begin_consent(None).await.unwrap();
            f.worker.approve_consent(approved).await.unwrap();
            assert!(f.worker.record(Event::PlanViewed {}).await.unwrap());
            f.worker.flush().await.unwrap();
            assert!(!f.recording.lock().unwrap().batches.is_empty());
            f.recording.lock().unwrap().batches.clear();
            let explicit = stage != 0;
            let port = transport(match stage {
                0 => vec![response.clone()],
                1 => vec![json!({"revision":0}), response.clone()],
                _ => vec![
                    json!({"revision":0}),
                    json!({"status":"applied"}),
                    response.clone(),
                ],
            });
            assert!(f
                .sync(&port, explicit.then_some(Consent::On))
                .await
                .is_err());
            assert_eq!(f.worker.status().await.unwrap().consent, Consent::Off);
            f.no_delivery().await;
            assert_eq!(port.calls.lock().len(), stage + 1);
        }
    }
}

#[tokio::test]
async fn offline_withdrawal_retries_original_account_and_device_after_restart() {
    let f = Fixture::new().await;
    let device = f.worker.device_id().await.unwrap();
    let offline = transport(vec![]);
    offline
        .replies
        .lock()
        .push_back(Err(CallableFailure::Offline));
    assert!(f.sync(&offline, Some(Consent::Off)).await.is_err());
    let pending = f.worker.consent_ticket().await.unwrap().withdrawals;
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].uid, "account-X");
    assert_eq!(pending[0].device_id, device);
    f.no_delivery().await;
    let restored = worker(f._dir.path().join("settings.json"), f.recording.clone());
    restored
        .account_changed(Some("account-X".into()))
        .await
        .unwrap();
    let port = transport(vec![json!({"status":"applied"})]);
    let ticket = restored.begin_consent(None).await.unwrap();
    synchronize(
        &Bridge::default(),
        &restored,
        &port,
        "fixture-token",
        "account-X",
        ticket,
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        port.calls.lock()[0].1,
        json!({"state":"off","change_id":pending[0].change_id})
    );
    assert!(restored
        .consent_ticket()
        .await
        .unwrap()
        .withdrawals
        .is_empty());
    assert_eq!(restored.status().await.unwrap().consent, Consent::Off);
    assert!(!restored.record(Event::PlanViewed {}).await.unwrap());
    restored.flush().await.unwrap();
    assert!(f.recording.lock().unwrap().batches.is_empty());
}

#[tokio::test]
async fn offline_withdrawal_survives_other_account_and_retries_on_original_sign_in() {
    let f = Fixture::new().await;
    let offline = transport(vec![]);
    offline
        .replies
        .lock()
        .push_back(Err(CallableFailure::Offline));
    assert!(f.sync(&offline, Some(Consent::Off)).await.is_err());
    let pending = f.worker.consent_ticket().await.unwrap().withdrawals;
    f.worker
        .account_changed(Some("account-Y".into()))
        .await
        .unwrap();
    let port = transport(vec![]);
    let ticket = f.worker.begin_consent(None).await.unwrap();
    synchronize(
        &f.bridge,
        &f.worker,
        &port,
        "fixture-token",
        "account-Y",
        ticket,
        None,
    )
    .await
    .unwrap();
    assert!(port.calls.lock().is_empty());
    assert_eq!(
        f.worker.consent_ticket().await.unwrap().withdrawals,
        pending
    );
    f.worker.account_changed(None).await.unwrap();
    f.worker
        .account_changed(Some("account-X".into()))
        .await
        .unwrap();
    let port = transport(vec![json!({"status":"applied"})]);
    f.sync(&port, None).await.unwrap();
    assert_eq!(
        port.calls.lock()[0].1["change_id"],
        pending[0].change_id.to_string()
    );
    assert!(f
        .worker
        .consent_ticket()
        .await
        .unwrap()
        .withdrawals
        .is_empty());
    f.no_delivery().await;
}

#[tokio::test]
async fn in_flight_on_cannot_override_newer_off_or_account_switch() {
    for boundary in 0..3 {
        for switch in [false, true] {
            let f = Fixture::new().await;
            let (entered, reached) = oneshot::channel();
            let (release, resume) = oneshot::channel();
            let port = transport(vec![
                json!({"revision":0}),
                json!({"status":"applied"}),
                json!({"status":"recorded"}),
            ]);
            let ticket = f.worker.begin_consent(Some(Consent::On)).await.unwrap();
            struct Barrier<'a> {
                port: &'a Transport,
                remaining: Mutex<usize>,
                entered: Mutex<Option<oneshot::Sender<()>>>,
                release: Mutex<Option<oneshot::Receiver<()>>>,
            }
            #[async_trait::async_trait]
            impl CallableTransport for Barrier<'_> {
                async fn call(
                    &self,
                    route: &str,
                    token: &str,
                    data: Value,
                ) -> Result<Value, CallableFailure> {
                    let block = {
                        let mut left = self.remaining.lock();
                        if *left == 0 {
                            true
                        } else {
                            *left -= 1;
                            false
                        }
                    };
                    if block {
                        let entered = self.entered.lock().take();
                        let release = self.release.lock().take();
                        if let Some(entered) = entered {
                            entered.send(()).unwrap();
                        }
                        if let Some(release) = release {
                            release.await.unwrap();
                        }
                    }
                    self.port.call(route, token, data).await
                }
            }
            let barrier = Barrier {
                port: &port,
                remaining: Mutex::new(boundary),
                entered: Mutex::new(Some(entered)),
                release: Mutex::new(Some(resume)),
            };
            let old_on = synchronize(
                &f.bridge,
                &f.worker,
                &barrier,
                "fixture-token",
                "account-X",
                ticket,
                Some(Consent::On),
            );
            let newer = async {
                reached.await.unwrap();
                if switch {
                    f.worker
                        .account_changed(Some("account-Y".into()))
                        .await
                        .unwrap();
                } else {
                    f.worker.begin_consent(Some(Consent::Off)).await.unwrap();
                }
                release.send(()).unwrap();
            };
            let (outcome, ()) = tokio::join!(old_on, newer);
            assert!(outcome.is_err());
            assert_eq!(port.calls.lock().len(), boundary + 1);
            f.no_delivery().await;
            if !switch {
                let off = transport(vec![json!({"status":"applied"})]);
                f.sync(&off, None).await.unwrap();
                assert_eq!(off.calls.lock()[0].1["state"], "off");
                assert_eq!(f.worker.status().await.unwrap().consent, Consent::Off);
            }
        }
    }
}

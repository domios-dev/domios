use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{json, Value};

use super::*;

struct FakeRuntime {
    owner: Mutex<String>,
    resume: Option<SurfaceResume>,
}

#[async_trait::async_trait]
impl SurfaceRuntime for FakeRuntime {
    fn set_owner_name(&self, name: &str) {
        *self.owner.lock() = name.to_string();
    }

    fn state(&self) -> SurfaceState {
        SurfaceState {
            owner: self.owner.lock().clone(),
            url: self
                .resume
                .as_ref()
                .and_then(|resume| resume.url.clone())
                .unwrap_or_default(),
            ..Default::default()
        }
    }

    async fn connect(self: Arc<Self>, _viewport: Option<ViewportRequest>) -> SurfaceEvents {
        Box::pin(futures_util::stream::empty())
    }

    async fn call(
        &self,
        _ctx: &SurfaceCallCtx,
        method: &str,
        _args: Value,
    ) -> Result<SurfaceCallOutcome, String> {
        Ok(SurfaceCallOutcome::value(json!(method)))
    }

    async fn agent_verb(
        &self,
        _ctx: &SurfaceCallCtx,
        verb: &str,
        _args: &[String],
    ) -> Result<String, String> {
        Ok(verb.to_string())
    }

    fn annotation_delivered(&self, _id: &str) {}

    async fn dispose(&self) {}
}

struct FakeFactory;

impl SurfaceRuntimeFactory for FakeFactory {
    fn create(
        &self,
        owner_name: &str,
        _owner_scope: &str,
        resume: Option<SurfaceResume>,
    ) -> Arc<dyn SurfaceRuntime> {
        Arc::new(FakeRuntime {
            owner: Mutex::new(owner_name.to_string()),
            resume,
        })
    }
}

fn registry() -> SurfaceRuntimeRegistry {
    let mut factories: HashMap<String, Arc<dyn SurfaceRuntimeFactory>> = HashMap::new();
    factories.insert("fake".into(), Arc::new(FakeFactory));
    SurfaceRuntimeRegistry::new(factories)
}

#[test]
fn surface_runtime_registry_keeps_one_instance_per_tab() {
    let registry = registry();
    let (first, created) = registry
        .get_or_create("t1", "fake", "Jev", "scope", None)
        .unwrap();
    assert!(created);
    let (again, created) = registry
        .get_or_create("t1", "fake", "Other", "scope", None)
        .unwrap();
    assert!(!created);
    assert!(Arc::ptr_eq(&first, &again));
    assert_eq!(again.state().owner, "Jev");
    let (other, _) = registry
        .get_or_create("t2", "fake", "Ann", "scope", None)
        .unwrap();
    assert!(!Arc::ptr_eq(&first, &other));
}

#[test]
fn surface_runtime_registry_seeds_resume_only_into_a_new_instance() {
    let registry = registry();
    let resume = |url: &str| {
        Some(SurfaceResume {
            url: Some(url.into()),
            profile: None,
        })
    };
    let (first, _) = registry
        .get_or_create("t1", "fake", "Jev", "scope", resume("https://a.dev/"))
        .unwrap();
    assert_eq!(first.state().url, "https://a.dev/");
    let (again, created) = registry
        .get_or_create("t1", "fake", "Jev", "scope", resume("https://b.dev/"))
        .unwrap();
    assert!(!created);
    assert_eq!(again.state().url, "https://a.dev/");
}

#[test]
fn surface_runtime_registry_refuses_an_unregistered_runtime() {
    let registry = registry();
    let error = match registry.get_or_create("t1", "missing", "Jev", "scope", None) {
        Err(error) => error,
        Ok(_) => panic!("unregistered runtime created"),
    };
    assert_eq!(error.code(), "surface_runtime_unavailable");
    assert_eq!(registry.runtime_names(), vec!["fake".to_string()]);
}

#[test]
fn surface_runtime_registry_hands_back_instances_of_closed_tabs() {
    let registry = registry();
    registry
        .get_or_create("live", "fake", "Jev", "scope", None)
        .unwrap();
    registry
        .get_or_create("gone", "fake", "Jev", "scope", None)
        .unwrap();
    let closed = registry.take_closed(|tab_id| tab_id == "live");
    assert_eq!(closed.len(), 1);
    assert!(registry.take_closed(|_| true).is_empty());
    assert_eq!(registry.take_closed(|_| false).len(), 1);
}

#[test]
fn surface_runtime_builtin_registry_names_the_browser_runtime() {
    let root = tempfile::tempdir().unwrap();
    let registry = SurfaceRuntimeRegistry::with_builtin_runtimes(root.path());
    assert_eq!(registry.runtime_names(), vec!["browser".to_string()]);
}

#[test]
fn surface_evidence_falls_back_to_the_data_dir_without_an_owner_pad() {
    let root = tempfile::tempdir().unwrap();
    let fallback = root.path().join("browsers").join("screenshots");
    let mut host = crate::test_support::RecordingSurfaceHost::default();
    assert_eq!(evidence_dir(&host, root.path(), None), fallback);
    assert_eq!(
        evidence_dir(&host, root.path(), Some("tab-without-pad")),
        fallback
    );
    let pad = root.path().join("runtime").join("agent-pads").join("tab-1");
    host.pads.insert("tab-1".into(), pad.clone());
    assert_eq!(
        evidence_dir(&host, root.path(), Some("tab-1")),
        pad.join("screenshots")
    );
    assert_eq!(
        *host.calls.lock(),
        vec!["agent_pad:tab-without-pad", "agent_pad:tab-1"]
    );
}

#[test]
fn surface_state_serializes_the_stream_state_shape() {
    let state = SurfaceState {
        owner: "Jev".into(),
        target_id: "target-1".into(),
        needs_human: "none".into(),
        profiles: vec!["work".into()],
        ..Default::default()
    };
    let value = serde_json::to_value(&state).unwrap();
    for key in [
        "owner",
        "title",
        "url",
        "activity",
        "needsHuman",
        "profile",
        "profiles",
    ] {
        assert!(value.get(key).is_some(), "missing {key}");
    }
    assert_eq!(value["targetId"], "target-1");
    assert!(serde_json::to_value(SurfaceState::default())
        .unwrap()
        .get("targetId")
        .is_none());
    assert_eq!(
        serde_json::to_value(SurfaceNotice::error("x")).unwrap(),
        json!({ "text": "x", "tone": "error" })
    );
}

#[test]
fn surface_sse_payload_names_events_and_base64s_frames() {
    use codeterm_browser::screencast::{FrameMetadata, ScreencastFrame};
    use codeterm_browser::viewport::Viewport;
    let frame = Arc::new(ScreencastFrame {
        seq: 1,
        jpeg: b"jpg".to_vec(),
        metadata: FrameMetadata::still(Viewport::default(), 12.5),
        still: true,
    });
    let (name, data) = sse_payload(&SurfaceEvent::Frame(frame));
    assert_eq!(name, "frame");
    let data: Value = serde_json::from_str(&data).unwrap();
    assert_eq!(data["data"], "anBn");
    assert_eq!(data["metadata"]["deviceWidth"], json!(1600.0));
    assert_eq!(data["metadata"]["timestamp"], json!(12.5));
    let (name, data) = sse_payload(&SurfaceEvent::Notice(SurfaceNotice::info("hi")));
    assert_eq!(
        (name, data.as_str()),
        ("notice", r#"{"text":"hi","tone":"info"}"#)
    );
    let (name, _) = sse_payload(&SurfaceEvent::State(SurfaceState::default()));
    assert_eq!(name, "state");
}

#[test]
fn surface_notice_preview_paused_carries_kind_and_technical_detail() {
    assert_eq!(
        serde_json::to_value(SurfaceNotice::preview_paused("no frame: x")).unwrap(),
        json!({ "text": PREVIEW_PAUSED_TEXT, "kind": "preview_paused", "detail": "no frame: x" })
    );
    assert_eq!(
        serde_json::to_value(SurfaceNotice::plain("x")).unwrap(),
        json!({ "text": "x" }),
        "untyped notices keep their wire shape"
    );
}

enum Teardown {
    Graceful(std::time::Duration),
    KilledAfterGrace,
    Hangs,
}

struct ClosingRuntime(Teardown);

#[async_trait::async_trait]
impl SurfaceRuntime for ClosingRuntime {
    fn set_owner_name(&self, _name: &str) {}

    fn state(&self) -> SurfaceState {
        SurfaceState::default()
    }

    async fn connect(self: Arc<Self>, _viewport: Option<ViewportRequest>) -> SurfaceEvents {
        Box::pin(futures_util::stream::empty())
    }

    async fn call(
        &self,
        _ctx: &SurfaceCallCtx,
        _method: &str,
        _args: Value,
    ) -> Result<SurfaceCallOutcome, String> {
        Ok(SurfaceCallOutcome::value(Value::Null))
    }

    async fn agent_verb(
        &self,
        _ctx: &SurfaceCallCtx,
        _verb: &str,
        _args: &[String],
    ) -> Result<String, String> {
        Ok(String::new())
    }

    fn annotation_delivered(&self, _id: &str) {}

    async fn dispose(&self) {}

    async fn shutdown(&self, graceful: std::time::Duration) -> bool {
        match self.0 {
            Teardown::Graceful(takes) => {
                tokio::time::sleep(takes).await;
                true
            }
            Teardown::KilledAfterGrace => {
                tokio::time::sleep(graceful).await;
                false
            }
            Teardown::Hangs => std::future::pending().await,
        }
    }
}

#[tokio::test(start_paused = true)]
async fn surface_shutdown_all_is_parallel_and_bounded() {
    use std::time::Duration;
    let runtimes: Vec<Arc<dyn SurfaceRuntime>> = vec![
        Arc::new(ClosingRuntime(Teardown::Graceful(Duration::from_millis(
            1500,
        )))),
        Arc::new(ClosingRuntime(Teardown::KilledAfterGrace)),
        Arc::new(ClosingRuntime(Teardown::Hangs)),
    ];
    let started = tokio::time::Instant::now();
    let report = shutdown_all(runtimes, Duration::from_secs(2), Duration::from_secs(4)).await;
    assert_eq!(
        report,
        ShutdownReport {
            graceful: 1,
            killed: 2
        }
    );
    assert_eq!(started.elapsed(), Duration::from_secs(4));
}

#[tokio::test(start_paused = true)]
async fn surface_shutdown_all_without_runtimes_returns_at_once() {
    let started = tokio::time::Instant::now();
    let report = shutdown_all(
        Vec::new(),
        std::time::Duration::from_secs(2),
        std::time::Duration::from_secs(4),
    )
    .await;
    assert_eq!(report, ShutdownReport::default());
    assert_eq!(started.elapsed(), std::time::Duration::ZERO);
}

#[test]
fn surface_runtime_registry_take_all_drains_every_instance() {
    let registry = registry();
    registry
        .get_or_create("t1", "fake", "Jev", "scope", None)
        .unwrap();
    registry
        .get_or_create("t2", "fake", "Ann", "scope", None)
        .unwrap();
    assert_eq!(registry.take_all().len(), 2);
    assert!(registry.take_all().is_empty());
    let (_, created) = registry
        .get_or_create("t1", "fake", "Jev", "scope", None)
        .unwrap();
    assert!(created);
}

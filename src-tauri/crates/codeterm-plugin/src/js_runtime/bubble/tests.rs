use super::*;
use crate::view::Tone;

impl JsBubbleProvider {
    async fn render_for_test(&self, ctx: Value) -> Result<Option<BubbleData>, BubbleError> {
        let (_, worker) = self.request(ctx.clone());
        worker.unwrap().await.unwrap();
        self.bubble(ctx).await
    }
}

fn vm_with(src: &str) -> Arc<PluginVm> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let id = format!("test-bubble-{}", NEXT.fetch_add(1, Ordering::Relaxed));
    Arc::new(PluginVm::from_source(&id, src.to_string()).expect("vm"))
}

#[tokio::test]
async fn returns_pill() {
    let vm = vm_with(
        r#"module.exports.default = { statusBubble() { return { label: "main", tone: "ok" }; } }"#,
    );
    let b = JsBubbleProvider::new(vm, "test-bubble");
    let pill = b
        .render_for_test(serde_json::json!({}))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(pill.label, "main");
    assert_eq!(pill.tone, Tone::Ok);
}

#[tokio::test]
async fn null_hides_the_pill() {
    let vm = vm_with(r#"module.exports.default = { statusBubble() { return null; } }"#);
    let b = JsBubbleProvider::new(vm, "test-bubble");
    assert!(b
        .render_for_test(serde_json::json!({}))
        .await
        .unwrap()
        .is_none());
}

const CONFINED_SRC: &str = r#"module.exports.default = {
    statusBubble(ctx) {
        var names = host.listChildDirs(ctx.cwd || ctx.probe);
        return { label: names === null ? "denied" : names.join(",") };
    }
}"#;

fn confined_vm() -> Arc<PluginVm> {
    use crate::js_runtime::host_api::PluginSandbox;
    Arc::new(
        PluginVm::from_source_sandboxed(
            "confined-bubble",
            CONFINED_SRC.to_string(),
            None,
            Arc::new(PluginSandbox::restricted(vec![], vec![])),
        )
        .expect("vm"),
    )
}

/// The host-assembled bubble ctx carries the tab cwd; dispatching the call
/// must record it as a sandbox read-root so discovery under it works.
#[tokio::test]
async fn host_ctx_cwd_becomes_sandbox_read_root() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("repo").join(".git")).unwrap();
    let b = JsBubbleProvider::new(confined_vm(), "confined-bubble");
    let pill = b
        .render_for_test(serde_json::json!({ "cwd": tmp.path().to_string_lossy() }))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(pill.label, "repo");
}

/// NEGATIVE CONTROL: a ctx without `cwd` mints no grant — the same listing
/// against the same path is denied on a fresh sandbox.
#[tokio::test]
async fn ctx_without_cwd_grants_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("repo").join(".git")).unwrap();
    let b = JsBubbleProvider::new(confined_vm(), "confined-bubble");
    let pill = b
        .render_for_test(serde_json::json!({ "probe": tmp.path().to_string_lossy() }))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(pill.label, "denied");
}

#[tokio::test]
async fn slow_status_returns_cached_value_and_releases_vm_until_completion() {
    use crate::js_runtime::call_gate;
    use serde_json::json;
    use std::time::Duration;

    let entered = "bubble-background.entered";
    let release = "bubble-background.release";
    call_gate::reset(entered);
    call_gate::reset(release);
    struct ReleaseOnDrop;
    impl Drop for ReleaseOnDrop {
        fn drop(&mut self) {
            call_gate::open("bubble-background.release");
        }
    }
    let _release_on_drop = ReleaseOnDrop;
    let vm = vm_with(
        r#"module.exports.default = {
        statusBubble() {
            var job = host.testGateStart("bubble-background.release");
            host.testGateOpen("bubble-background.entered");
            return host.awaitJob(job, function () { return { label: "fresh" }; });
        },
        fast() { return 7; }
    }"#,
    );
    let provider = JsBubbleProvider::new(vm.clone(), "test-bubble");
    provider.cache.lock().request("repo", false);
    provider.cache.lock().finish(
        "repo",
        Some(Some(BubbleData {
            label: "last known".into(),
            tone: Tone::Default,
            icon: None,
            tooltip: None,
            action: None,
        })),
    );
    let (last, worker) = provider.request(json!({ "cwd": "repo", "refresh": true }));
    assert_eq!(last.unwrap().label, "last known");
    tokio::task::spawn_blocking(move || {
        assert!(call_gate::wait_opened(entered, Duration::from_secs(30)));
    })
    .await
    .unwrap();
    let last = provider
        .bubble(json!({ "cwd": "repo" }))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(last.label, "last known");
    let fast = tokio::task::spawn_blocking(move || call_json::<u32>(&vm, "fast", &[]));
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(30), fast)
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
        7
    );
    assert!(!call_gate::is_open(release));
    assert!(!worker.as_ref().unwrap().is_finished());
    call_gate::open(release);
    worker.unwrap().await.unwrap();
    assert_eq!(
        provider
            .bubble(json!({ "cwd": "repo" }))
            .await
            .unwrap()
            .unwrap()
            .label,
        "fresh"
    );
}

use super::*;

fn vm_with(src: &str) -> Arc<PluginVm> {
    Arc::new(PluginVm::from_source("test-transcriber", src.to_string()).expect("vm"))
}

#[tokio::test]
async fn transcribe_returns_text() {
    let vm = vm_with(
        r#"module.exports.default = {
            transcribe(path, lang) { return { text: "hello " + lang }; }
        }"#,
    );
    let t = JsTranscriber::new(vm, "test");
    let out = t.transcribe(Path::new("/a.oga"), "ru").await.unwrap();
    assert_eq!(out.text, "hello ru");
}

#[tokio::test]
async fn empty_text_is_error() {
    let vm = vm_with(r#"module.exports.default = { transcribe() { return { text: "  " }; } }"#);
    let t = JsTranscriber::new(vm, "test");
    assert!(matches!(
        t.transcribe(Path::new("/a.oga"), "").await,
        Err(TranscribeError::Empty)
    ));
}

#[tokio::test]
async fn plugin_error_propagates() {
    let vm = vm_with(
        r#"module.exports.default = { transcribe() { return { error: "engine down" }; } }"#,
    );
    let t = JsTranscriber::new(vm, "test");
    assert!(matches!(
        t.transcribe(Path::new("/a.oga"), "").await,
        Err(TranscribeError::RequestFailed(_))
    ));
}

// --- Real shipped plugin: load it, prove capabilities + the host.fetch path ---

fn transcriber_manifest() -> std::path::PathBuf {
    crate::test_support::fixture_manifest_dir()
        .join("..")
        .join("plugins/transcriber/plugin.json")
}

#[test]
fn shipped_transcriber_manifest_declares_capability() {
    let mp = transcriber_manifest();
    if !mp.exists() {
        return; // plugins/ not present in this checkout
    }
    let p = super::super::loader::load_one(&mp).expect("load transcriber");
    assert_eq!(p.manifest().id, "transcriber");
    assert!(p.manifest().capabilities.transcriber);
    assert!(
        !p.manifest().capabilities.agent_provider,
        "transcriber-only"
    );
    assert_eq!(p.manifest().host_api, Some(2));
}

// Multi-thread: the plugin's host.fetch blocks a worker on its request thread;
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shipped_transcriber_transcribes_via_fake_daemon() {
    use axum::{routing::post, Json, Router};
    let mp = transcriber_manifest();
    if !mp.exists() {
        return;
    }
    // Fake transcribe daemon.
    let router = Router::new().route(
        "/transcribe",
        post(|| async { Json(serde_json::json!({ "text": "salut le monde" })) }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                let _ = rx.await;
            })
            .await;
    });
    let p = super::super::loader::load_one(&mp).expect("load transcriber");
    let t = JsTranscriber::new(p.vm().clone(), p.manifest().id.clone());
    let out = {
        let mut env = codeterm_util::test_env::EnvGuard::lock();
        env.set("CODETERM_TRANSCRIBE_URL", format!("http://{addr}"));
        futures_executor::block_on(t.transcribe(Path::new("/note.oga"), "fr"))
    };

    let _ = tx.send(());
    let _ = handle.await;

    assert!(
        matches!(&out, Ok(t) if t.text == "salut le monde"),
        "got {out:?}"
    );
}

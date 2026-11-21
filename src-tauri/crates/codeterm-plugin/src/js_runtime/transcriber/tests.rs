use super::*;

#[tokio::test]
async fn live_dictation_requires_the_declared_js_capability() {
    let vm = vm_with(r#"module.exports.default = {
      dictationLaunch() { return {state:"setup", reason:"missing fixture model"}; }
    }"#);
    let clip_only = JsTranscriber::new(vm.clone(), "test");
    assert!(!clip_only.supports_dictation());
    assert!(matches!(clip_only.start_dictation().await, DictationAdmission::Unavailable(_)));
    let live = JsTranscriber::new(vm, "test").with_dictation(true);
    assert!(live.supports_dictation());
    assert!(matches!(live.start_dictation().await, DictationAdmission::Setup(reason) if reason == "missing fixture model"));
}

#[test]
fn dictation_process_frames_have_one_bounded_binary_request_and_revisable_reply() {
    use std::io::Cursor;
    let mut written = Vec::new();
    let mut reply = Cursor::new(b"{\"committed\":\"\",\"provisional\":\"interim speech\"}\n");
    let segment = session::exchange(&mut written, &mut reply, &[7, 8]).unwrap();
    assert_eq!(written, [2, 0, 0, 0, 7, 8]);
    assert!(segment.committed.is_empty());
    assert_eq!(segment.provisional, "interim speech");
    let mut written = Vec::new();
    let mut reply = Cursor::new(b"{\"committed\":\"final speech\",\"provisional\":\"\"}\n");
    let segment = session::exchange(&mut written, &mut reply, &[]).unwrap();
    assert_eq!(written, [0; 4]);
    assert_eq!(segment.committed, "final speech");
    let mut oversized = Cursor::new(vec![b' '; codeterm_types::dictation::MAX_UPDATE_BYTES + 1]);
    assert!(session::exchange(&mut Vec::new(), &mut oversized, &[7, 8]).is_err());
}

#[tokio::test]
async fn dictation_glance_gets_the_loaded_bundle_and_keeps_legacy_argument_positions() {
    use crate::glance::GlanceProvider;
    let directory = tempfile::tempdir().unwrap();
    let entry = directory.path().join("plugin.js");
    for has_dictation in [false, true] {
        let declaration = if has_dictation { "dictationLaunch() {}," } else { "" };
        std::fs::write(&entry, format!(r#"module.exports.default = {{
          {declaration}
          renderGlance(ctx, own) {{
            return {{title: JSON.stringify([ctx.fixture, own || null, arguments.length]), nodes:[]}};
          }},
          glanceAction(action, ctx, own) {{
            return {{title: JSON.stringify([action, ctx.fixture, own || null, arguments.length]), nodes:[]}};
          }}
        }}"#)).unwrap();
        let vm = Arc::new(PluginVm::load("fixture-glance", &entry).unwrap());
        let provider = super::super::JsGlanceProvider::new(vm, "fixture-glance");
        let ctx = json!({"fixture":"scope", "bundle":"caller-controlled"});
        let expected = has_dictation.then(|| directory.path().to_string_lossy().into_owned());
        let render = provider.render(ctx.clone()).await.unwrap();
        let action = provider.action("activate", ctx).await.unwrap();
        assert_eq!(render.title, Some(json!(["scope", expected, if has_dictation {2} else {1}]).to_string()));
        assert_eq!(action.title, Some(json!(["activate", "scope", expected, if has_dictation {3} else {2}]).to_string()));
    }
}

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

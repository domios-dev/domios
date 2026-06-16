//! `JsTranscriber` — adapts a JS plugin that declares `capabilities.transcriber`
//! to the core [`Transcriber`](crate::transcribe::Transcriber) trait. Mirrors
//! `JsProvider`: it wraps the shared `Arc<PluginVm>` and dispatches to exported
//! plugin functions via the synchronous `call_json` (the VM is not `Send`, so we
//! call it directly on the caller's thread — gap G1).

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use super::vm::{call_json, PluginVm};
use crate::transcribe::{TranscribeError, Transcriber, TranscriberStatus, Transcript};

pub struct JsTranscriber {
    vm: Arc<PluginVm>,
    id: String,
}

impl JsTranscriber {
    pub fn new(vm: Arc<PluginVm>, id: impl Into<String>) -> Self {
        Self { vm, id: id.into() }
    }
}

#[derive(Deserialize)]
struct TranscribeOut {
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Deserialize)]
struct StatusOut {
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    reason: Option<String>,
}

#[async_trait]
impl Transcriber for JsTranscriber {
    fn id(&self) -> &str {
        &self.id
    }

    async fn status(&self) -> TranscriberStatus {
        // `transcribeStatus` is optional; absent/error ⇒ assume Ready.
        match call_json::<StatusOut>(&self.vm, "transcribeStatus", &[]) {
            Ok(s) => match s.state.as_deref() {
                Some("loading") => TranscriberStatus::Loading,
                Some("unloaded") => TranscriberStatus::Unloaded,
                Some("unavailable") => TranscriberStatus::Unavailable {
                    reason: s.reason.unwrap_or_default(),
                },
                _ => TranscriberStatus::Ready,
            },
            Err(_) => TranscriberStatus::Ready,
        }
    }

    async fn transcribe(
        &self,
        audio_path: &Path,
        language: &str,
    ) -> Result<Transcript, TranscribeError> {
        let args = [
            json!(audio_path.to_string_lossy().to_string()),
            json!(language),
        ];
        match call_json::<TranscribeOut>(&self.vm, "transcribe", &args) {
            Ok(out) => {
                if let Some(e) = out.error {
                    return Err(TranscribeError::RequestFailed(e));
                }
                let text = out.text.unwrap_or_default().trim().to_string();
                if text.is_empty() {
                    Err(TranscribeError::Empty)
                } else {
                    Ok(Transcript { text })
                }
            }
            Err(e) => Err(TranscribeError::RequestFailed(format!(
                "plugin transcribe failed: {e:?}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
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
        let vm = vm_with(
            r#"module.exports.default = { transcribe() { return { text: "  " }; } }"#,
        );
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
        std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/.."))
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
        assert!(!p.manifest().capabilities.agent_provider, "transcriber-only");
        assert_eq!(p.manifest().host_api, Some(2));
    }

    // Multi-thread: the plugin's host.fetch blocks a worker on its request thread;
    // the in-runtime fake daemon needs another worker to respond. In production the
    // daemon is a separate process, so a single worker is fine there.
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
        // Point the plugin at the fake daemon. (Workspace plugins get an
        // unrestricted sandbox, so the allowlist doesn't block the random port.)
        std::env::set_var("CODETERM_TRANSCRIBE_URL", format!("http://{addr}"));

        let p = super::super::loader::load_one(&mp).expect("load transcriber");
        let t = JsTranscriber::new(p.vm().clone(), p.manifest().id.clone());
        let out = t.transcribe(Path::new("/note.oga"), "fr").await;

        std::env::remove_var("CODETERM_TRANSCRIBE_URL");
        let _ = tx.send(());
        let _ = handle.await;

        assert!(matches!(&out, Ok(t) if t.text == "salut le monde"), "got {out:?}");
    }
}

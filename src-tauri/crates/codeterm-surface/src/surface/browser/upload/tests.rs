use super::*;
use codeterm_browser::cdp::{CdpClient, CdpConnection, CdpError, EventStream};
use codeterm_browser::cdp_wire::{CdpEvent, EventFilter};
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::Duration;

#[test]
fn upload_paths_refuse_escape_devices_streams_and_ambiguous_names() {
    let root = tempfile::tempdir().unwrap();
    assert_eq!(
        relative_path(root.path(), "video.mp4").unwrap(),
        Path::new("video.mp4")
    );
    for bad in [
        "",
        "../secret",
        "x/../../secret",
        "NUL",
        "NUL.mp4",
        "NUL .txt",
        "CON.txt",
        "CONIN$",
        "CONOUT$",
        "COM\u{00b9}",
        "LPT\u{00b2}.txt",
        "COM1",
        "LPT9.txt",
        "x:stream",
        "x.",
        "x ",
        "x\u{0001}",
    ] {
        assert!(relative_path(root.path(), bad).is_err(), "{bad}");
    }
    let outside = tempfile::tempdir().unwrap();
    assert!(relative_path(root.path(), outside.path().join("file").to_str().unwrap()).is_err());
}

fn context(root: &Path) -> SurfaceCallCtx {
    let mut host = crate::test_support::RecordingSurfaceHost::default();
    host.pads.insert("caller".into(), root.join("caller"));
    host.pads.insert("owner".into(), root.join("owner"));
    SurfaceCallCtx {
        host: Arc::new(host),
        caller_tab_id: Some("caller".into()),
        owner_tab_id: Some("owner".into()),
        data_dir: root.into(),
        actor: None,
    }
}

#[cfg(windows)]
#[test]
fn upload_admission_pins_25mb_file_and_uses_only_the_callers_pad() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("caller")).unwrap();
    std::fs::create_dir(root.path().join("owner")).unwrap();
    let path = root.path().join("caller/video.mp4");
    File::create(&path)
        .unwrap()
        .set_len(25 * 1024 * 1024)
        .unwrap();
    let ctx = context(root.path());
    let admitted = admit(&ctx, &["video.mp4".into()]).unwrap();
    assert_eq!(admitted[0].size, 26_214_400);
    assert_eq!(admitted[0].name, "video.mp4");
    assert!(std::fs::write(&path, b"replacement").is_err());
    assert!(std::fs::rename(&path, root.path().join("caller/replaced.mp4")).is_err());
    assert!(admit(
        &ctx,
        &[root
            .path()
            .join("owner/video.mp4")
            .to_string_lossy()
            .into_owned()]
    )
    .is_err());
    let mut unattributed = ctx.clone();
    unattributed.caller_tab_id = None;
    assert!(admit(&unattributed, &["video.mp4".into()]).is_err());
    drop(admitted);
    std::fs::write(path, b"released").unwrap();
}

#[cfg(windows)]
#[test]
fn upload_refuses_junction_ancestors() {
    use std::process::Command;
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("caller")).unwrap();
    std::fs::create_dir(root.path().join("outside")).unwrap();
    std::fs::write(root.path().join("outside/secret.txt"), b"private").unwrap();
    // mklink rejects forward slashes, so build native separators.
    let link = root.path().join("caller").join("linked");
    let status = Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(&link)
        .arg(root.path().join("outside"))
        .output()
        .unwrap();
    assert!(
        status.status.success(),
        "{}",
        String::from_utf8_lossy(&status.stderr)
    );
    let result = admit(&context(root.path()), &["linked/secret.txt".into()]);
    std::fs::remove_dir(link).unwrap();
    assert!(result.is_err());
}

struct Connection {
    direct: bool,
    mode: &'static str,
    intercept_fails: bool,
    calls: Mutex<Vec<(String, Value)>>,
    events: Mutex<Option<tokio::sync::mpsc::Sender<Arc<CdpEvent>>>>,
    closed: tokio::sync::watch::Sender<Option<String>>,
}
#[async_trait::async_trait]
impl CdpConnection for Connection {
    fn endpoint(&self) -> &str {
        "ws://127.0.0.1:1/upload"
    }
    fn closed(&self) -> tokio::sync::watch::Receiver<Option<String>> {
        self.closed.subscribe()
    }
    fn close(&self, _: &str) {}
    fn subscribe(&self, _: EventFilter) -> EventStream {
        let (tx, rx) = tokio::sync::mpsc::channel(4);
        *self.events.lock() = Some(tx);
        EventStream::from_receiver(rx)
    }
    async fn send_to(
        &self,
        _: Option<&str>,
        method: &str,
        params: Value,
        _: Duration,
    ) -> Result<Value, CdpError> {
        self.calls.lock().push((method.into(), params.clone()));
        Ok(match method {
            "Target.attachToTarget" => json!({"sessionId":"s"}),
            "Page.setInterceptFileChooserDialog" if self.intercept_fails => {
                return Err(CdpError::Deadline(method.into()))
            }
            "DOM.describeNode" => {
                if self.direct {
                    json!({"node":{"nodeName":"INPUT","attributes":["type","file"]}})
                } else {
                    json!({"node":{"nodeName":"BUTTON"}})
                }
            }
            "DOM.getContentQuads" => json!({"quads":[[0,0,20,0,20,20,0,20]]}),
            "Input.dispatchMouseEvent" if params["type"] == "mouseReleased" => {
                self.events
                    .lock()
                    .as_ref()
                    .unwrap()
                    .try_send(Arc::new(CdpEvent {
                        method: "Page.fileChooserOpened".into(),
                        params: json!({"backendNodeId":42,"mode":self.mode}),
                        session_id: Some("s".into()),
                    }))
                    .unwrap();
                json!({})
            }
            "DOM.resolveNode" => json!({"object":{"objectId":"file-input"}}),
            "Runtime.callFunctionOn" => {
                if params["functionDeclaration"]
                    .as_str()
                    .unwrap()
                    .contains("this.files")
                {
                    json!({"result":{"value":vec![json!({"name":"video.mp4","size":26214400});if self.mode == "selectMultiple" {2} else {1}]}})
                } else {
                    json!({"result":{"value":{"file":true,"multiple":self.mode == "selectMultiple","origin":"https://upload.example"}}})
                }
            }
            _ => json!({}),
        })
    }
}

#[tokio::test]
async fn upload_intercepts_before_click_and_sets_the_chooser_node_without_native_fallback() {
    for (direct, mode, fails, count) in [
        (false, "selectSingle", false, 1),
        (true, "selectSingle", false, 1),
        (false, "selectMultiple", false, 2),
        (false, "selectSingle", true, 1),
        (false, "selectSingle", false, 2),
    ] {
        let connection = Arc::new(Connection {
            direct,
            mode,
            intercept_fails: fails,
            calls: Mutex::new(Vec::new()),
            events: Mutex::new(None),
            closed: tokio::sync::watch::channel(None).0,
        });
        let page = CdpClient::from_connection(connection.clone())
            .attach_page("target")
            .await
            .unwrap();
        let root = tempfile::tempdir().unwrap();
        let runtime = BrowserRuntime::new(
            codeterm_browser::BrowserHostConfig::under_data_dir(root.path()),
            "Agent",
            "tab",
            Arc::new(tokio::sync::OnceCell::new()),
            Default::default(),
        );
        let mut notices = runtime.shared.subscribe_notices();
        let paths = vec!["admitted/video.mp4".to_string(); count];
        let result = runtime
            .upload_files(
                &page,
                7,
                &paths,
                &vec![json!({"name":"video.mp4","size":26214400}); count],
                "video.mp4 (26214400 bytes)",
                None,
            )
            .await;
        let calls = connection.calls.lock();
        let intercept = calls
            .iter()
            .position(|(method, _)| method == "Page.setInterceptFileChooserDialog")
            .unwrap();
        let selected = calls
            .iter()
            .position(|(method, _)| method == "DOM.setFileInputFiles");
        if fails || (count > 1 && mode != "selectMultiple") {
            assert!(result.is_err());
            assert!(selected.is_none());
            if fails {
                assert!(!calls
                    .iter()
                    .any(|(method, _)| method == "Input.dispatchMouseEvent"));
            }
        } else {
            assert!(result.unwrap().contains("https://upload.example"));
            let selected = selected.unwrap();
            assert!(intercept < selected);
            assert_eq!(
                calls[selected].1,
                json!({"files":paths,"backendNodeId":if direct {7} else {42}})
            );
            assert_eq!(
                notices.try_recv().unwrap().text,
                "uploading video.mp4 (26214400 bytes) to https://upload.example"
            );
            if !direct {
                assert!(
                    intercept
                        < calls
                            .iter()
                            .position(|(method, _)| method == "Input.dispatchMouseEvent")
                            .unwrap()
                );
            }
        }
    }
}

#[tokio::test]
async fn rejected_upload_paths_never_reach_cdp() {
    let connection = Arc::new(Connection {
        direct: true,
        mode: "selectSingle",
        intercept_fails: false,
        calls: Mutex::new(Vec::new()),
        events: Mutex::new(None),
        closed: tokio::sync::watch::channel(None).0,
    });
    let page = CdpClient::from_connection(connection.clone())
        .attach_page("target")
        .await
        .unwrap();
    connection.calls.lock().clear();
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("caller")).unwrap();
    let runtime = BrowserRuntime::new(
        codeterm_browser::BrowserHostConfig::under_data_dir(root.path()),
        "Agent",
        "tab",
        Arc::new(tokio::sync::OnceCell::new()),
        Default::default(),
    );
    assert!(runtime
        .upload_verb(
            &context(root.path()),
            &page,
            &["@opaque-ref".into(), "../outside.mp4".into()]
        )
        .await
        .is_err());
    assert!(connection.calls.lock().is_empty());
    assert!(runtime.uploads.lock().is_empty());
}

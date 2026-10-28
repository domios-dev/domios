use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{json, Value};

use super::{open_download, perform, HostActionRequest, MediaRef};
use codeterm_service_core::plugin::js_runtime::ViewHostAction;
use codeterm_service_core::plugin_media::{MediaRefusal, OpenedMedia, PluginMediaResolver};
use codeterm_service_core::view_host::{ViewError, ViewProvider, ViewRegistry};

struct StubView {
    id: &'static str,
    actions: Vec<ViewHostAction>,
}

#[async_trait::async_trait]
impl ViewProvider for StubView {
    fn id(&self) -> &str {
        self.id
    }

    fn host_actions(&self) -> &[ViewHostAction] {
        &self.actions
    }

    async fn call(&self, _method: &str, _args: Value) -> Result<Value, ViewError> {
        Ok(Value::Null)
    }
}

/// Serves one file to the frame of its owning plugin; any other plugin or frame gets `Foreign`.
struct StubMedia {
    owner: &'static str,
    file: PathBuf,
}

impl PluginMediaResolver for StubMedia {
    fn open(&self, plugin_id: &str, frame: &str, handle: &str) -> Result<OpenedMedia, MediaRefusal> {
        if handle == EXPIRED {
            return Err(MediaRefusal::Expired);
        }
        if handle != HANDLE {
            return Err(MediaRefusal::Unknown);
        }
        if plugin_id != self.owner || frame != FRAME {
            return Err(MediaRefusal::Foreign);
        }
        Ok(OpenedMedia {
            file: std::fs::File::open(&self.file).expect("fixture media"),
            path: self.file.clone(),
            file_name: "frame.png".into(),
            content_type: "image/png".into(),
        })
    }
}

const HANDLE: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const EXPIRED: &str = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
const FRAME: &str = "frame-generation-1";

fn media_ref<'a>(plugin_id: &'a str, frame: &'a str) -> MediaRef<'a> {
    MediaRef {
        plugin_id,
        frame: Some(frame),
        handle: Some(HANDLE),
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("domios-host-action-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn registry() -> ViewRegistry {
    let registry = ViewRegistry::new();
    registry.register(
        "studio",
        Arc::new(StubView {
            id: "studio",
            actions: vec![ViewHostAction::CopyText, ViewHostAction::SaveAs, ViewHostAction::Reveal],
        }),
    );
    registry.register("quiet", Arc::new(StubView { id: "quiet", actions: Vec::new() }));
    registry.register(
        "other",
        Arc::new(StubView {
            id: "other",
            actions: vec![ViewHostAction::SaveAs],
        }),
    );
    registry
}

fn media(dir: &std::path::Path) -> StubMedia {
    let file = dir.join("media.png");
    std::fs::write(&file, b"png-bytes").expect("fixture file");
    StubMedia { owner: "studio", file }
}

fn request(plugin_id: &str, action: ViewHostAction) -> HostActionRequest {
    HostActionRequest {
        plugin_id: plugin_id.into(),
        action,
        handle: Some(HANDLE.into()),
        frame: Some(FRAME.into()),
        destination: None,
    }
}

fn refusal(code: &str, reason: &str) -> Value {
    json!({ "ok": false, "code": code, "reason": reason })
}

#[test]
fn host_action_request_refuses_unknown_actions_and_fields() {
    let parse = |body: Value| serde_json::from_value::<HostActionRequest>(body);
    assert!(parse(json!({ "pluginId": "studio", "action": "copyText" })).is_ok());
    assert!(parse(json!({ "pluginId": "studio", "action": "exec" })).is_err());
    assert!(parse(json!({ "pluginId": "studio", "action": "reveal", "path": "/etc/hosts" })).is_err());
}

#[test]
fn host_action_is_denied_unless_the_plugin_declared_it() {
    let registry = registry();
    assert_eq!(
        perform(&registry, None, request("quiet", ViewHostAction::CopyText)),
        refusal("denied", "undeclared")
    );
    assert_eq!(
        perform(&registry, None, request("other", ViewHostAction::Reveal)),
        refusal("denied", "undeclared")
    );
    assert_eq!(
        perform(&registry, None, request("absent", ViewHostAction::CopyText)),
        refusal("unavailable", "view")
    );
    assert_eq!(
        perform(&registry, None, request("studio", ViewHostAction::CopyText)),
        json!({ "ok": true })
    );
}

#[test]
fn media_actions_are_unavailable_until_a_resolver_is_installed() {
    let registry = registry();
    assert_eq!(
        perform(&registry, None, request("studio", ViewHostAction::Reveal)),
        refusal("unavailable", "media_handles")
    );
}

#[test]
fn media_actions_refuse_malformed_and_foreign_handles_without_reading() {
    let dir = scratch("handles");
    let registry = registry();
    let resolver = media(&dir);
    for handle in [None, Some("../media.png".to_string()), Some(HANDLE.to_uppercase())] {
        let mut reveal = request("studio", ViewHostAction::Reveal);
        reveal.handle = handle;
        assert_eq!(perform(&registry, Some(&resolver), reveal), refusal("invalid", "handle"));
    }
    let mut expired = request("studio", ViewHostAction::Reveal);
    expired.handle = Some(EXPIRED.into());
    assert_eq!(
        perform(&registry, Some(&resolver), expired),
        refusal("denied", "media_expired")
    );
    assert_eq!(
        open_download(&registry, Some(&resolver), media_ref("other", FRAME)).err(),
        Some(refusal("denied", "media_foreign"))
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn media_actions_need_the_frame_that_owns_the_handle() {
    let dir = scratch("frames");
    let registry = registry();
    let resolver = media(&dir);
    let mut frameless = request("studio", ViewHostAction::Reveal);
    frameless.frame = None;
    assert_eq!(perform(&registry, Some(&resolver), frameless), refusal("invalid", "frame"));
    let mut reloaded = request("studio", ViewHostAction::Reveal);
    reloaded.frame = Some("frame-generation-2".into());
    assert_eq!(
        perform(&registry, Some(&resolver), reloaded),
        refusal("denied", "media_foreign")
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn reveal_returns_the_resolved_path_for_the_file_manager() {
    let dir = scratch("reveal");
    let resolver = media(&dir);
    let result = perform(&registry(), Some(&resolver), request("studio", ViewHostAction::Reveal));
    assert_eq!(result["ok"], json!(true));
    assert_eq!(result["path"], json!(resolver.file.to_string_lossy()));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn save_as_copies_the_opened_file_to_the_chosen_destination() {
    let dir = scratch("save");
    let resolver = media(&dir);
    let destination = dir.join("picked.png");
    std::fs::write(&destination, b"older").expect("existing file the dialog confirmed");
    let mut save = request("studio", ViewHostAction::SaveAs);
    save.destination = Some(destination.to_string_lossy().into_owned());
    assert_eq!(perform(&registry(), Some(&resolver), save), json!({ "ok": true }));
    assert_eq!(std::fs::read(&destination).unwrap(), b"png-bytes");
    assert!(!dir.join(".picked.png.domios-save").exists());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn save_as_refuses_relative_folder_and_missing_destinations() {
    let dir = scratch("destination");
    let resolver = media(&dir);
    let registry = registry();
    for destination in [
        None,
        Some(String::new()),
        Some("picked.png".to_string()),
        Some(dir.to_string_lossy().into_owned()),
        Some(dir.join("missing").join("picked.png").to_string_lossy().into_owned()),
    ] {
        let mut save = request("studio", ViewHostAction::SaveAs);
        save.destination = destination;
        assert_eq!(perform(&registry, Some(&resolver), save), refusal("invalid", "destination"));
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn download_needs_the_save_as_declaration() {
    let dir = scratch("download");
    let resolver = media(&dir);
    let registry = registry();
    assert_eq!(
        open_download(&registry, Some(&resolver), media_ref("quiet", FRAME)).err(),
        Some(refusal("denied", "undeclared"))
    );
    let opened = open_download(&registry, Some(&resolver), media_ref("studio", FRAME)).expect("declared and owned");
    assert_eq!(opened.content_type, "image/png");
    drop(opened);
    let _ = std::fs::remove_dir_all(dir);
}

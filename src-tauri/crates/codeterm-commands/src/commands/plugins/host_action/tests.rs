use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{json, Value};

use super::{open_download, perform, settle_ingest, HostActionRequest, MediaRef};
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

    async fn call(
        &self,
        _method: &str,
        _args: Value,
        _context: &codeterm_plugin::view_host::ViewCallContext,
    ) -> Result<Value, ViewError> {
        Ok(Value::Null)
    }
}

/// Serves one file to the frame of its owning plugin; any other plugin or frame gets `Foreign`.
struct StubMedia {
    owner: &'static str,
    file: PathBuf,
    /// The length media delivery validated at registration; the file on disk may since differ.
    registered: Option<u64>,
}

impl PluginMediaResolver for StubMedia {
    fn open(
        &self,
        plugin_id: &str,
        frame: &str,
        handle: &str,
    ) -> Result<OpenedMedia, MediaRefusal> {
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
            size: self
                .registered
                .unwrap_or_else(|| std::fs::metadata(&self.file).expect("fixture media").len()),
            file: std::fs::File::open(&self.file).expect("fixture media"),
            path: self.file.clone(),
            file_name: "frame.png".into(),
            content_type: "image/png".into(),
        })
    }
}

const HANDLE: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const EXPIRED: &str = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
const FRAME: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const RELOADED_FRAME: &str = "2222222222222222222222222222222222222222222222222222222222222222";

fn media_ref<'a>(plugin_id: &'a str, frame: &'a str) -> MediaRef<'a> {
    MediaRef {
        plugin_id,
        frame: Some(frame),
        handle: Some(HANDLE),
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("domios-host-action-{name}-{}", std::process::id()));
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
            actions: vec![
                ViewHostAction::CopyText,
                ViewHostAction::SaveAs,
                ViewHostAction::Reveal,
            ],
        }),
    );
    registry.register(
        "quiet",
        Arc::new(StubView {
            id: "quiet",
            actions: Vec::new(),
        }),
    );
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
    StubMedia {
        owner: "studio",
        file,
        registered: None,
    }
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
    assert!(
        parse(json!({ "pluginId": "studio", "action": "reveal", "path": "/etc/hosts" })).is_err()
    );
    // The frame comes from the verified lease; a body naming one is refused, not trusted.
    assert!(parse(json!({ "pluginId": "studio", "action": "reveal", "frame": "1".repeat(64) })).is_err());
}

/// A view that declared pickFile, with or without (valid) inputs.
struct PickingView(Option<codeterm_plugin::media_delivery::ViewInputDescriptor>);

#[async_trait::async_trait]
impl ViewProvider for PickingView {
    fn id(&self) -> &str {
        "picker"
    }

    fn host_actions(&self) -> &[ViewHostAction] {
        &[ViewHostAction::PickFile]
    }

    fn view_inputs(&self) -> Option<codeterm_plugin::media_delivery::ViewInputDescriptor> {
        self.0.clone()
    }

    async fn call(
        &self,
        _method: &str,
        _args: Value,
        _context: &codeterm_plugin::view_host::ViewCallContext,
    ) -> Result<Value, ViewError> {
        Ok(Value::Null)
    }
}

fn ingested(handle: &str) -> codeterm_plugin::media_delivery::IngestedInput {
    codeterm_plugin::media_delivery::IngestedInput {
        handle: handle.into(),
        file_name: "fox.png".into(),
        content_type: "image/png".into(),
        size: 3,
        sha256: "0".repeat(64),
    }
}

#[test]
fn ingest_answers_only_the_opaque_handle_and_its_display_fields() {
    let answer = settle_ingest(Ok(ingested(HANDLE)));
    assert_eq!(
        answer,
        json!({ "ok": true, "input": { "handle": HANDLE, "fileName": "fox.png", "contentType": "image/png", "size": 3 } })
    );
    assert!(!answer.to_string().contains(&"0".repeat(64)), "the digest stays host-side");
}

#[test]
fn ingest_refusals_keep_their_typed_reasons() {
    use codeterm_plugin::media_delivery::IngestRefusal;
    // UnknownFrame includes a document that stopped being current during the copy, which
    // ingestion itself cleaned up (media delivery's two-frame cleanup fixture covers that).
    for (refusal_kind, expected) in [
        (IngestRefusal::UnknownFrame, refusal("denied", "stale")),
        (IngestRefusal::TooLarge, refusal("invalid", "too_large")),
        (IngestRefusal::TypeNotAccepted, refusal("invalid", "type")),
        (IngestRefusal::Unreadable, refusal("unavailable", "unreadable")),
        (IngestRefusal::Storage, refusal("unavailable", "storage")),
        (IngestRefusal::Deadline, refusal("unavailable", "deadline")),
    ] {
        assert_eq!(settle_ingest(Err(refusal_kind)), expected);
    }
}

#[test]
fn pick_file_preflight_answers_only_the_declared_bounds() {
    let registry = ViewRegistry::new();
    let declared = codeterm_plugin::media_delivery::ViewInputDescriptor {
        accept: vec!["image/png".into(), "image/jpeg".into()],
        max_bytes: 1024,
        max_files: 2,
    };
    registry.register("picker", Arc::new(PickingView(Some(declared))));
    registry.register("bare", Arc::new(PickingView(None)));
    assert_eq!(
        perform(&registry, None, request("picker", ViewHostAction::PickFile)),
        json!({ "ok": true, "accept": ["image/png", "image/jpeg"], "maxBytes": 1024, "maxFiles": 2 })
    );
    assert_eq!(
        perform(&registry, None, request("bare", ViewHostAction::PickFile)),
        refusal("denied", "undeclared")
    );
}

#[test]
fn save_as_copies_only_the_registered_length_of_a_file_that_grew() {
    let dir = scratch("grown");
    let mut resolver = media(&dir);
    // Registered as "png" (3 bytes); the plugin kept appending afterwards.
    resolver.registered = Some(3);
    let destination = dir.join("picked.png");
    let mut save = request("studio", ViewHostAction::SaveAs);
    save.destination = Some(destination.to_string_lossy().into_owned());
    assert_eq!(perform(&registry(), Some(&resolver), save), json!({ "ok": true }));
    assert_eq!(std::fs::read(&destination).unwrap(), b"png");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn save_as_refuses_a_file_cut_short_after_admission_and_keeps_the_destination() {
    let dir = scratch("shrunk");
    let mut resolver = media(&dir);
    resolver.registered = Some(4096);
    let destination = dir.join("picked.png");
    std::fs::write(&destination, b"kept").unwrap();
    let mut save = request("studio", ViewHostAction::SaveAs);
    save.destination = Some(destination.to_string_lossy().into_owned());
    assert_eq!(
        perform(&registry(), Some(&resolver), save),
        refusal("unavailable", "media_changed")
    );
    assert_eq!(std::fs::read(&destination).unwrap(), b"kept");
    assert_eq!(leftover_temps(&dir), 0);
    let _ = std::fs::remove_dir_all(dir);
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
    for handle in [
        None,
        Some("../media.png".to_string()),
        Some(HANDLE.to_uppercase()),
    ] {
        let mut reveal = request("studio", ViewHostAction::Reveal);
        reveal.handle = handle;
        assert_eq!(
            perform(&registry, Some(&resolver), reveal),
            refusal("invalid", "handle")
        );
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
    assert_eq!(
        perform(&registry, Some(&resolver), frameless),
        refusal("invalid", "frame")
    );
    let mut malformed = request("studio", ViewHostAction::Reveal);
    malformed.frame = Some("frame-generation-1".into());
    assert_eq!(
        perform(&registry, Some(&resolver), malformed),
        refusal("invalid", "frame")
    );
    let mut reloaded = request("studio", ViewHostAction::Reveal);
    reloaded.frame = Some(RELOADED_FRAME.into());
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
    let result = perform(
        &registry(),
        Some(&resolver),
        request("studio", ViewHostAction::Reveal),
    );
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
    assert_eq!(
        perform(&registry(), Some(&resolver), save),
        json!({ "ok": true })
    );
    assert_eq!(std::fs::read(&destination).unwrap(), b"png-bytes");
    assert_eq!(leftover_temps(&dir), 0);
    let _ = std::fs::remove_dir_all(dir);
}

fn leftover_temps(dir: &std::path::Path) -> usize {
    std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".domios-save-")
        })
        .count()
}

#[test]
fn save_as_never_touches_files_it_did_not_create() {
    let dir = scratch("siblings");
    let resolver = media(&dir);
    let destination = dir.join("picked.png");
    // A file at the name an earlier fixed-name scheme would have used, and the user's own file.
    let sibling = dir.join(".picked.png.domios-save");
    std::fs::write(&sibling, b"not ours").unwrap();
    std::fs::write(&destination, b"kept on refusal").unwrap();

    let mut foreign = request("other", ViewHostAction::SaveAs);
    foreign.destination = Some(destination.to_string_lossy().into_owned());
    assert_eq!(
        perform(&registry(), Some(&resolver), foreign),
        refusal("denied", "media_foreign")
    );
    assert_eq!(std::fs::read(&destination).unwrap(), b"kept on refusal");

    let mut save = request("studio", ViewHostAction::SaveAs);
    save.destination = Some(destination.to_string_lossy().into_owned());
    assert_eq!(
        perform(&registry(), Some(&resolver), save),
        json!({ "ok": true })
    );
    assert_eq!(std::fs::read(&sibling).unwrap(), b"not ours");
    assert_eq!(std::fs::read(&destination).unwrap(), b"png-bytes");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn concurrent_saves_to_one_destination_each_use_their_own_temp() {
    let dir = scratch("concurrent");
    let resolver = Arc::new(media(&dir));
    let destination = dir.join("picked.png");
    let workers: Vec<_> = (0..4)
        .map(|_| {
            let resolver = resolver.clone();
            let destination = destination.clone();
            std::thread::spawn(move || {
                let mut save = request("studio", ViewHostAction::SaveAs);
                save.destination = Some(destination.to_string_lossy().into_owned());
                perform(&registry(), Some(resolver.as_ref()), save)
            })
        })
        .collect();
    // Racing renames onto one name may lose on some file systems; a loser must refuse cleanly.
    let results: Vec<Value> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    assert!(results.contains(&json!({ "ok": true })), "{results:?}");
    for result in &results {
        assert!(
            *result == json!({ "ok": true }) || *result == refusal("unavailable", "write_failed"),
            "{result}"
        );
    }
    assert_eq!(std::fs::read(&destination).unwrap(), b"png-bytes");
    assert_eq!(leftover_temps(&dir), 0);
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
        Some(
            dir.join("missing")
                .join("picked.png")
                .to_string_lossy()
                .into_owned(),
        ),
    ] {
        let mut save = request("studio", ViewHostAction::SaveAs);
        save.destination = destination;
        assert_eq!(
            perform(&registry, Some(&resolver), save),
            refusal("invalid", "destination")
        );
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
    let opened = open_download(&registry, Some(&resolver), media_ref("studio", FRAME))
        .expect("declared and owned");
    assert_eq!(opened.content_type, "image/png");
    drop(opened);
    let _ = std::fs::remove_dir_all(dir);
}

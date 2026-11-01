//! Host side of a plugin view's user-gesture actions (`ct.host.*`).
//!
//! The frontend has already verified the user gesture and stamped the plugin id;
//! this layer enforces what only the daemon can: the action is one the plugin
//! declared, and media is the plugin's own file, opened by the media resolver.
//! Refusals are typed results (`{ ok: false, code, reason }`), never errors.

use std::path::{Path, PathBuf};

use codeterm_plugin::media_delivery::{
    self, IngestLimits, IngestRefusal, IngestSource, ViewInputDescriptor,
};
use codeterm_plugin::view_host::ViewCallContext;
use codeterm_service_core::plugin::js_runtime::ViewHostAction;
use codeterm_service_core::plugin_media::{
    valid_media_handle, FrameMedia, MediaRefusal, OpenedMedia, PluginMediaResolver,
};
use codeterm_service_core::view_host::{ViewRegistry, ViewRegistryHandle};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct HostActionRequest {
    pub plugin_id: String,
    pub action: ViewHostAction,
    #[serde(default)]
    pub handle: Option<String>,
    /// The asking document's frame, resolved by the daemon from the app-stamped private lease;
    /// never read from the body (a body naming it is refused as an unknown field).
    #[serde(skip)]
    pub frame: Option<String>,
    /// `saveAs` only: the path the user picked in the native save dialog.
    #[serde(default)]
    pub destination: Option<String>,
}

fn refused(code: &str, reason: &str) -> Value {
    json!({ "ok": false, "code": code, "reason": reason })
}

fn media_refusal(refusal: MediaRefusal) -> Value {
    refused(
        "denied",
        match refusal {
            MediaRefusal::Unknown => "media_unknown",
            MediaRefusal::Expired => "media_expired",
            MediaRefusal::Foreign => "media_foreign",
        },
    )
}

fn declared(registry: &ViewRegistry, plugin_id: &str, action: ViewHostAction) -> Result<(), Value> {
    let provider = registry
        .get(plugin_id)
        .ok_or_else(|| refused("unavailable", "view"))?;
    if provider.host_actions().contains(&action) {
        Ok(())
    } else {
        Err(refused("denied", "undeclared"))
    }
}

/// Which plugin, which iframe generation and which handle a media action names.
#[derive(Clone, Copy)]
pub(super) struct MediaRef<'a> {
    pub plugin_id: &'a str,
    pub frame: Option<&'a str>,
    pub handle: Option<&'a str>,
}

fn open_media(
    resolver: Option<&dyn PluginMediaResolver>,
    media: MediaRef<'_>,
) -> Result<OpenedMedia, Value> {
    let handle = media
        .handle
        .filter(|handle| valid_media_handle(handle))
        .ok_or_else(|| refused("invalid", "handle"))?;
    let frame = media
        .frame
        .filter(|frame| valid_media_handle(frame))
        .ok_or_else(|| refused("invalid", "frame"))?;
    let resolver = resolver.ok_or_else(|| refused("unavailable", "media_handles"))?;
    resolver
        .open(media.plugin_id, frame, handle)
        .map_err(media_refusal)
}

/// An absolute file path in an existing folder that is not a folder or a link itself.
fn save_destination(raw: Option<&str>) -> Result<PathBuf, Value> {
    let invalid = || refused("invalid", "destination");
    let path = PathBuf::from(
        raw.filter(|raw| !raw.trim().is_empty())
            .ok_or_else(invalid)?,
    );
    if !path.is_absolute() || path.file_name().is_none() {
        return Err(invalid());
    }
    let parent = path.parent().ok_or_else(invalid)?;
    if !std::fs::metadata(parent).is_ok_and(|meta| meta.is_dir()) {
        return Err(invalid());
    }
    match std::fs::symlink_metadata(&path) {
        Ok(meta) if !meta.file_type().is_file() => Err(invalid()),
        _ => Ok(path),
    }
}

fn write_failed(error: std::io::Error) -> Value {
    log::warn!("[plugin] host action saveAs failed: {error}");
    refused("unavailable", "write_failed")
}

/// Copy exactly the registered length of the already-opened media into a fresh, randomly named
/// sibling temp file this call creates, then rename it over the destination. Bytes the plugin
/// appended after registration are never read; a file cut short after admission is refused.
/// A failed copy never leaves a truncated file where the user asked for one, and cleanup only
/// ever removes that temp file.
fn copy_to(media: OpenedMedia, destination: &Path) -> Result<(), Value> {
    let folder = destination
        .parent()
        .ok_or_else(|| refused("invalid", "destination"))?;
    let mut partial = tempfile::Builder::new()
        .prefix(".domios-save-")
        .tempfile_in(folder)
        .map_err(write_failed)?;
    let mut registered = std::io::Read::take(media.file, media.size);
    let copied = std::io::copy(&mut registered, partial.as_file_mut()).map_err(write_failed)?;
    if copied != media.size {
        return Err(refused("unavailable", "media_changed"));
    }
    partial.as_file().sync_all().map_err(write_failed)?;
    partial
        .persist(destination)
        .map(drop)
        .map_err(|error| write_failed(error.error))
}

pub(super) fn perform(
    registry: &ViewRegistry,
    resolver: Option<&dyn PluginMediaResolver>,
    request: HostActionRequest,
) -> Value {
    if let Err(refusal) = declared(registry, &request.plugin_id, request.action) {
        return refusal;
    }
    let media = MediaRef {
        plugin_id: &request.plugin_id,
        frame: request.frame.as_deref(),
        handle: request.handle.as_deref(),
    };
    let result = match request.action {
        // The frontend writes the text; the daemon only confirms the declaration.
        ViewHostAction::CopyText => Ok(json!({ "ok": true })),
        // Media is checked before the destination, so a request without one is the app's
        // preflight: `invalid / destination` means everything but the user's choice is in order.
        ViewHostAction::SaveAs => open_media(resolver, media).and_then(|opened| {
            copy_to(opened, &save_destination(request.destination.as_deref())?)?;
            Ok(json!({ "ok": true }))
        }),
        ViewHostAction::Reveal => open_media(resolver, media)
            .map(|opened| json!({ "ok": true, "path": opened.path.to_string_lossy() })),
        // The app's preflight before it opens the picker: the declared kinds and bounds, so the
        // dialog can filter. Files themselves only ever come in through the ingest route.
        ViewHostAction::PickFile => registry
            .get(&request.plugin_id)
            .and_then(|provider| provider.view_inputs())
            .filter(ViewInputDescriptor::valid)
            .map(|inputs| {
                json!({
                    "ok": true,
                    "accept": inputs.accept,
                    "maxBytes": inputs.max_bytes,
                    "maxFiles": inputs.max_files,
                })
            })
            .ok_or_else(|| refused("denied", "undeclared")),
    };
    result.unwrap_or_else(|refusal| refusal)
}

/// `saveAs` without a native dialog (the web mirror): the declared, opened file to stream as a download.
pub(super) fn open_download(
    registry: &ViewRegistry,
    resolver: Option<&dyn PluginMediaResolver>,
    media: MediaRef<'_>,
) -> Result<OpenedMedia, Value> {
    declared(registry, media.plugin_id, ViewHostAction::SaveAs)?;
    open_media(resolver, media)
}

/// What the app stamped on a host-action request, from the asking document's loader: its private
/// lease and its registration (presentation) key. Neither ever comes from the frame.
#[derive(Clone, Copy)]
pub struct DocumentStamp<'a> {
    pub lease: Option<&'a str>,
    pub registration: Option<&'a str>,
}

/// The asking document, proven by its private lease: the lease resolves to a live registration
/// whose key is the stamped one, served by the plugin's current view provider, in a context the
/// service still accepts. A retired, reminted or replaced document answers `denied / stale`.
fn leased_frame(
    svc: &codeterm_service_core::service::ServiceState,
    plugin_id: &str,
    stamp: DocumentStamp<'_>,
) -> Result<(String, ViewCallContext), Value> {
    let stale = || refused("denied", "stale");
    let key = stamp
        .registration
        .filter(|key| valid_media_handle(key))
        .ok_or_else(stale)?;
    let (provider, context) = media_delivery::view_frame(plugin_id, stamp.lease.ok_or_else(stale)?)
        .map_err(|_| stale())?;
    let current = svc
        .ext::<ViewRegistryHandle>()
        .get(plugin_id)
        .ok_or_else(stale)?;
    if !std::sync::Arc::ptr_eq(&provider, &current)
        || media_delivery::frame_context(plugin_id, key).map_err(|_| stale())? != context
        || svc.validate_plugin_view_context(&context).is_err()
    {
        return Err(stale());
    }
    Ok((key.to_owned(), context))
}

/// Shared by the desktop app and the web mirror (`POST /api/plugins/view-host-action`).
pub fn view_host_action(
    svc: &codeterm_service_core::service::ServiceState,
    mut request: HostActionRequest,
    stamp: DocumentStamp<'_>,
) -> Value {
    match leased_frame(svc, &request.plugin_id, stamp) {
        Ok((frame, _)) => request.frame = Some(frame),
        Err(refusal) => return refusal,
    }
    perform(svc.ext::<ViewRegistryHandle>(), Some(&FrameMedia), request)
}

/// Backs `POST /api/plugins/view-host-action/download`.
pub fn view_host_download(
    svc: &codeterm_service_core::service::ServiceState,
    plugin_id: &str,
    stamp: DocumentStamp<'_>,
    handle: Option<&str>,
) -> Result<OpenedMedia, Value> {
    let (frame, _) = leased_frame(svc, plugin_id, stamp)?;
    let media = MediaRef {
        plugin_id,
        frame: Some(&frame),
        handle,
    };
    open_download(svc.ext::<ViewRegistryHandle>(), Some(&FrameMedia), media)
}

/// Where a picked file comes from: the file the app's native open dialog chose, already opened
/// without following links (desktop), or the bounded request body (web mirror). Never a frame value.
pub enum PickedSource {
    Opened(std::fs::File),
    Body(Box<dyn std::io::Read + Send>),
}

fn ingest_refusal(refusal: IngestRefusal) -> Value {
    match refusal {
        IngestRefusal::UnknownFrame => refused("denied", "stale"),
        IngestRefusal::TooLarge => refused("invalid", "too_large"),
        IngestRefusal::TypeNotAccepted => refused("invalid", "type"),
        IngestRefusal::Unreadable => refused("unavailable", "unreadable"),
        IngestRefusal::Storage => refused("unavailable", "storage"),
        IngestRefusal::Deadline => refused("unavailable", "deadline"),
    }
}

/// A display name for a picked file: no separators, controls or reserved characters, never empty.
fn picked_name(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .filter(|c| {
            !c.is_control() && !matches!(c, '/' | '\\' | '"' | ':' | '*' | '?' | '<' | '>' | '|')
        })
        .take(120)
        .collect();
    let trimmed = cleaned.trim().trim_start_matches('.');
    if trimmed.is_empty() {
        "file".into()
    } else {
        trimmed.into()
    }
}

/// Backs `POST /api/plugins/view-input`: ingest one file the user picked into the plugin's own
/// inputs, bound to the asking document's verified context, and answer with its frame handle.
pub fn view_host_input(
    svc: &codeterm_service_core::service::ServiceState,
    plugin_id: &str,
    stamp: DocumentStamp<'_>,
    source: PickedSource,
    file_name: &str,
    declared_mime: &str,
) -> Value {
    let registry: &ViewRegistry = svc.ext::<ViewRegistryHandle>();
    if let Err(refusal) = declared(registry, plugin_id, ViewHostAction::PickFile) {
        return refusal;
    }
    let (frame, context) = match leased_frame(svc, plugin_id, stamp) {
        Ok(found) => found,
        Err(refusal) => return refusal,
    };
    let Some(inputs) = registry
        .get(plugin_id)
        .and_then(|provider| provider.view_inputs())
        .filter(ViewInputDescriptor::valid)
    else {
        return refused("denied", "undeclared");
    };
    let source = match source {
        PickedSource::Opened(file) => IngestSource::Opened(file),
        PickedSource::Body(reader) => IngestSource::Reader(reader),
    };
    let limits = IngestLimits {
        accept: &inputs.accept,
        max_bytes: inputs.max_bytes,
    };
    // The copy can take a while: ingestion asks this again after registering the handle and before
    // keeping the file, the same live check media registration does (lease, registration, provider
    // and the service scope: owner, workspace, Team). On `false` ingestion itself revokes that one
    // handle and its own output removes the file, so cleanup never reopens through current authority.
    let still_current = || {
        leased_frame(svc, plugin_id, stamp)
            .is_ok_and(|(current, now)| current == frame && now == context)
    };
    let outcome = media_delivery::ingest_frame_input(
        plugin_id,
        &frame,
        &context,
        source,
        &picked_name(file_name),
        declared_mime,
        limits,
        &still_current,
    );
    settle_ingest(outcome)
}

/// The answer for one ingestion, narrowed to what the view may see: never a path or a digest.
pub(super) fn settle_ingest(
    outcome: Result<media_delivery::IngestedInput, IngestRefusal>,
) -> Value {
    let input = match outcome {
        Ok(input) => input,
        Err(refusal) => return ingest_refusal(refusal),
    };
    json!({
        "ok": true,
        "input": {
            "handle": input.handle,
            "fileName": input.file_name,
            "contentType": input.content_type,
            "size": input.size,
        },
    })
}

#[cfg(test)]
mod tests;

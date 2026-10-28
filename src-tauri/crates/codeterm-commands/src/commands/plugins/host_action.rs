//! Host side of a plugin view's user-gesture actions (`ct.host.*`).
//!
//! The frontend has already verified the user gesture and stamped the plugin id;
//! this layer enforces what only the daemon can: the action is one the plugin
//! declared, and media is the plugin's own file, opened by the media resolver.
//! Refusals are typed results (`{ ok: false, code, reason }`), never errors.

use std::path::{Path, PathBuf};

use codeterm_service_core::plugin::js_runtime::ViewHostAction;
use codeterm_service_core::plugin_media::{
    valid_media_handle, MediaRefusal, OpenedMedia, PluginMediaResolver, PluginMediaResolverHandle,
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
    /// Host-held token of the iframe generation that asked; media must belong to it.
    #[serde(default)]
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

fn open_media(resolver: Option<&dyn PluginMediaResolver>, media: MediaRef<'_>) -> Result<OpenedMedia, Value> {
    let handle = media
        .handle
        .filter(|handle| valid_media_handle(handle))
        .ok_or_else(|| refused("invalid", "handle"))?;
    let frame = media
        .frame
        .filter(|frame| !frame.is_empty())
        .ok_or_else(|| refused("invalid", "frame"))?;
    let resolver = resolver.ok_or_else(|| refused("unavailable", "media_handles"))?;
    resolver
        .open(media.plugin_id, frame, handle)
        .map_err(media_refusal)
}

/// An absolute file path in an existing folder that is not a folder or a link itself.
fn save_destination(raw: Option<&str>) -> Result<PathBuf, Value> {
    let invalid = || refused("invalid", "destination");
    let path = PathBuf::from(raw.filter(|raw| !raw.trim().is_empty()).ok_or_else(invalid)?);
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

/// Copy the already-opened media into a sibling temp file, then rename it over the destination,
/// so a failed copy never leaves a truncated file where the user asked for one.
fn copy_to(mut media: OpenedMedia, destination: &Path) -> Result<(), Value> {
    let name = destination
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let partial = destination.with_file_name(format!(".{name}.domios-save"));
    let written = (|| {
        let mut out = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&partial)?;
        std::io::copy(&mut media.file, &mut out)?;
        out.sync_all()?;
        std::fs::rename(&partial, destination)
    })();
    written.map_err(|error| {
        let _ = std::fs::remove_file(&partial);
        log::warn!("[plugin] host action saveAs failed: {error}");
        refused("unavailable", "write_failed")
    })
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

fn resolver(svc: &codeterm_service_core::service::ServiceState) -> Option<&dyn PluginMediaResolver> {
    svc.try_ext::<PluginMediaResolverHandle>()
        .map(|handle| handle.0.as_ref())
}

/// Shared by the desktop app and the web mirror (`POST /api/plugins/view-host-action`).
pub fn view_host_action(svc: &codeterm_service_core::service::ServiceState, request: HostActionRequest) -> Value {
    perform(svc.ext::<ViewRegistryHandle>(), resolver(svc), request)
}

/// Backs `POST /api/plugins/view-host-action/download`.
pub fn view_host_download(
    svc: &codeterm_service_core::service::ServiceState,
    plugin_id: &str,
    frame: Option<&str>,
    handle: Option<&str>,
) -> Result<OpenedMedia, Value> {
    let media = MediaRef { plugin_id, frame, handle };
    open_download(svc.ext::<ViewRegistryHandle>(), resolver(svc), media)
}

#[cfg(test)]
mod tests;

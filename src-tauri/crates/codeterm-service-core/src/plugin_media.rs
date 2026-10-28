//! Seam between plugin view host actions and plugin-owned media files.
//!
//! Host actions only ever see a file media delivery has already opened under
//! the plugin's own data directory for the exact frame generation that owns the
//! handle, so they never reopen a path a symlink could have swapped.

pub use codeterm_plugin::media_delivery::{MediaRefusal, OpenedMedia};

pub trait PluginMediaResolver: Send + Sync {
    /// Open the file behind a host-issued handle for the plugin the host (never the frame) names,
    /// only when that exact frame generation owns the handle; a handle of another frame is `Foreign`.
    /// `frame` is the host-held token of the iframe that asked, never a value the frame supplies.
    fn open(&self, plugin_id: &str, frame: &str, handle: &str) -> Result<OpenedMedia, MediaRefusal>;
}

/// The production resolver: media delivery's atomic frame-ownership check and confined open.
pub struct FrameMedia;

impl PluginMediaResolver for FrameMedia {
    fn open(&self, plugin_id: &str, frame: &str, handle: &str) -> Result<OpenedMedia, MediaRefusal> {
        codeterm_plugin::media_delivery::open_frame_media(plugin_id, frame, handle)
    }
}

/// The handle and frame-token shape the host issues: 64 lowercase hex characters. Anything else is refused unread.
pub fn valid_media_handle(handle: &str) -> bool {
    codeterm_plugin::media_delivery::valid_token(handle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_handles_are_exactly_64_lowercase_hex() {
        assert!(valid_media_handle(&"a1".repeat(32)));
        assert!(!valid_media_handle(&"A1".repeat(32)));
        assert!(!valid_media_handle(&"a1".repeat(31)));
        assert!(!valid_media_handle(&format!("{}/", "a".repeat(63))));
        assert!(!valid_media_handle("../../etc/passwd"));
        assert!(!valid_media_handle(""));
    }

    #[test]
    fn frame_media_refuses_a_frame_the_store_never_issued() {
        let handle = "a".repeat(64);
        assert!(FrameMedia.open("studio", &"b".repeat(64), &handle).is_err());
    }
}

//! Seam between plugin view host actions and plugin-owned media files.
//!
//! The media delivery capability issues opaque handles and installs a
//! [`PluginMediaResolverHandle`] at startup; host actions only ever see a file
//! the resolver has already opened under the plugin's own data directory, so
//! they never reopen a path a symlink could have swapped. Until a resolver is
//! installed, media host actions answer `unavailable`.

use std::path::PathBuf;
use std::sync::Arc;

/// A plugin media file, opened and confined by the resolver.
pub struct OpenedMedia {
    pub file: std::fs::File,
    /// Shown to the file manager by `reveal`; never read or written through.
    pub path: PathBuf,
    /// Display name the plugin registered; host actions sanitize it again.
    pub file_name: String,
    pub content_type: String,
}

/// Why a handle does not open; deliberately says nothing about which plugin owns it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaRefusal {
    Unknown,
    Expired,
    Foreign,
}

pub trait PluginMediaResolver: Send + Sync {
    /// Open the file behind a host-issued handle for the plugin the host (never the frame) names,
    /// only when that exact frame generation owns the handle; a handle of another frame is `Foreign`.
    /// `frame` is the host-held token of the iframe that asked, never a value the frame supplies.
    fn open(&self, plugin_id: &str, frame: &str, handle: &str) -> Result<OpenedMedia, MediaRefusal>;
}

/// Service extension slot the media delivery capability fills.
pub struct PluginMediaResolverHandle(pub Arc<dyn PluginMediaResolver>);

/// The handle shape the host issues: 64 lowercase hex characters. Anything else is refused unread.
pub fn valid_media_handle(handle: &str) -> bool {
    handle.len() == 64 && handle.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
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
}

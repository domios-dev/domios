pub use async_trait::async_trait;

use crate::apply::{ApplyOptions, ApplyResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeUnavailable;

impl std::fmt::Display for NativeUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("native_capability_unavailable")
    }
}

impl std::error::Error for NativeUnavailable {}

#[async_trait::async_trait]
pub trait ApplicationHost: Send + Sync {
    fn available(&self) -> bool {
        true
    }
    fn restart(&self) -> Result<(), String>;
    fn exit(&self, code: i32) -> Result<(), String>;
    async fn copy_image_to_clipboard(&self, path: String) -> Result<(), String>;
    async fn check_and_apply(&self, options: ApplyOptions) -> Result<ApplyResult, String>;
}

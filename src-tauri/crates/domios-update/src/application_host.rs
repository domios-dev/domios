use std::path::PathBuf;

use crate::apply::{ApplyOptions, ApplyResult, DebianPreflight, DebianUpdatePlan};

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
    fn restart(&self) -> Result<(), String>;
    fn exit(&self, code: i32) -> Result<(), String>;
    async fn copy_image_to_clipboard(&self, path: String) -> Result<(), String>;
    fn debian_preflight(&self) -> DebianPreflight;
    async fn check_and_apply(&self, options: ApplyOptions) -> Result<ApplyResult, String>;
    async fn check_stage_deb_update(
        &self,
        options: &ApplyOptions,
    ) -> Result<DebianUpdatePlan, String>;
    async fn install_staged_deb(
        &self,
        version: String,
        path: PathBuf,
        initiating_tab: Option<String>,
    ) -> Result<(), String>;
}

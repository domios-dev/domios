//! App-supplied ports — traits defined here, implemented in the app crate.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use codeterm_types::config::schema::{TextHooksConfig, VoiceTranscribeConfig};

use crate::bridge::manager::OutboundSender;
use crate::transcribe::TranscribeRegistry;

pub use codeterm_plugin::chatpreprocessor::{
    ChatPreprocessorPort, ChatPreprocessorProvider, PreprocessCtx,
};

pub trait AttachmentStorePort: Send + Sync {
    fn secrets_dir(&self) -> PathBuf;
    fn evict_downloads(&self, dir: &Path, max_bytes: u64);
    fn telegram_allowed_users(&self) -> Vec<String>;
}

pub trait BridgeTabPort: Send + Sync {
    fn resolve_general_tab(&self) -> Result<String, String>;
    fn tab_agent_type(&self, tab_id: &str) -> Option<String>;
    fn transcribe_registry(&self) -> Arc<TranscribeRegistry>;
    fn bridge_outbound(&self) -> OutboundSender;
    fn voice_transcribe_config(&self) -> VoiceTranscribeConfig;
    fn text_hooks_config(&self) -> TextHooksConfig;
    fn deliver_inbound(
        &self,
        tab_id: &str,
        prefixed: &str,
        agent_type: Option<&str>,
    ) -> Result<(), String>;
    fn queue_inbound(&self, tab_id: &str, prefixed: String);
    fn telegram_token(&self) -> Option<String>;
    fn telegram_token_path(&self) -> PathBuf;
}

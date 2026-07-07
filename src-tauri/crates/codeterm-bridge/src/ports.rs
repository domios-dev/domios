//! App-supplied ports — traits defined here, implemented in the app crate.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use codeterm_types::config::schema::{TextHooksConfig, VoiceTranscribeConfig};

use crate::bridge::manager::OutboundSender;
use crate::transcribe::TranscribeRegistry;

/// Send-time text stage context (mirrors SDK `chatPreprocess(ctx)`).
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreprocessCtx {
    pub pane_id: String,
    pub cwd: Option<String>,
    pub provider: Option<String>,
    pub turn_index: u64,
}

/// A send-time text stage. Returns `Some(text)` to replace or `None` to pass through.
pub trait ChatPreprocessorProvider: Send + Sync {
    fn id(&self) -> &str;
    fn preprocess(&self, ctx: &PreprocessCtx, text: &str) -> Option<String>;
}

pub trait ChatPreprocessorPort: Send + Sync {
    fn preprocessor_ids(&self) -> Vec<String>;
    fn active_preprocessors(&self) -> Vec<Arc<dyn ChatPreprocessorProvider>>;
}

pub trait AttachmentStorePort: Send + Sync {
    fn secrets_dir(&self) -> PathBuf;
    fn evict_downloads(&self, dir: &Path, max_bytes: u64);
    fn telegram_allowed_users(&self) -> Vec<String>;
}

pub trait BridgePanePort: Send + Sync {
    fn resolve_general_pane(&self) -> Result<String, String>;
    fn pane_agent_type(&self, pane_id: &str) -> Option<String>;
    fn transcribe_registry(&self) -> Arc<TranscribeRegistry>;
    fn bridge_outbound(&self) -> OutboundSender;
    fn voice_transcribe_config(&self) -> VoiceTranscribeConfig;
    fn text_hooks_config(&self) -> TextHooksConfig;
    fn deliver_inbound(
        &self,
        pane_id: &str,
        prefixed: &str,
        agent_type: Option<&str>,
    ) -> Result<(), String>;
    fn queue_inbound(&self, pane_id: &str, prefixed: String);
    fn telegram_token(&self) -> Option<String>;
    fn telegram_token_path(&self) -> PathBuf;
}

impl ChatPreprocessorPort for crate::registry::CapabilityRegistry<dyn ChatPreprocessorProvider> {
    fn preprocessor_ids(&self) -> Vec<String> {
        self.ids()
    }

    fn active_preprocessors(&self) -> Vec<Arc<dyn ChatPreprocessorProvider>> {
        self.active()
    }
}

impl ChatPreprocessorPort for Arc<crate::registry::CapabilityRegistry<dyn ChatPreprocessorProvider>> {
    fn preprocessor_ids(&self) -> Vec<String> {
        self.ids()
    }

    fn active_preprocessors(&self) -> Vec<Arc<dyn ChatPreprocessorProvider>> {
        self.active()
    }
}

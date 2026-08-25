//! App-side implementations of [`codeterm_bridge::ports`] traits.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use codeterm_bridge::ports::{
    AttachmentStorePort, BridgeTabPort, ChatPreprocessorPort, ChatPreprocessorProvider,
};
use codeterm_bridge::transcribe::TranscribeRegistry;
use codeterm_types::config::schema::{TextHooksConfig, VoiceTranscribeConfig};

use crate::service::general_agent;
use crate::service::ServiceContext;

pub struct AppAttachmentStorePort;

pub fn attachment_store() -> Arc<dyn AttachmentStorePort> {
    Arc::new(AppAttachmentStorePort)
}

impl AttachmentStorePort for AppAttachmentStorePort {
    fn secrets_dir(&self) -> PathBuf {
        general_agent::secrets_dir()
    }

    fn evict_downloads(&self, dir: &Path, max_bytes: u64) {
        crate::attachments::evict(dir, crate::attachments::EvictionPolicy::MaxBytes(max_bytes));
    }

    fn telegram_allowed_users(&self) -> Vec<String> {
        crate::config::load_config()
            .integrations
            .telegram_allowed_users
            .clone()
    }
}

pub struct ServiceBridgeTabPort(pub ServiceContext);

impl BridgeTabPort for ServiceBridgeTabPort {
    fn resolve_general_tab(&self) -> Result<String, String> {
        self.0
            .mux
            .write()
            .resolve_tab_id(general_agent::GENERAL_ALIAS)
            .ok_or_else(|| "General Agent not found".to_string())
    }

    fn tab_agent_type(&self, tab_id: &str) -> Option<String> {
        self.0
            .mux
            .write()
            .get_tab(tab_id)
            .and_then(|p| p.agent_type.clone())
    }

    fn transcribe_registry(&self) -> Arc<TranscribeRegistry> {
        self.0.transcribe_registry.clone()
    }

    fn bridge_outbound(&self) -> codeterm_bridge::bridge::manager::OutboundSender {
        self.0.bridge_outbound.clone()
    }

    fn voice_transcribe_config(&self) -> VoiceTranscribeConfig {
        crate::config::load_config()
            .integrations
            .voice_transcribe
            .clone()
    }

    fn text_hooks_config(&self) -> TextHooksConfig {
        crate::config::load_config().integrations.text_hooks.clone()
    }

    fn deliver_inbound(
        &self,
        tab_id: &str,
        prefixed: &str,
        agent_type: Option<&str>,
    ) -> Result<(), String> {
        if agent_type.is_none() {
            let preview: String = prefixed.chars().take(120).collect();
            log::info!(
                "[bridge] queued inbound for tab={} (agent restarting): {}",
                tab_id,
                preview,
            );
            codeterm_bridge::bridge::inbound_buffer::push(tab_id, prefixed.to_string());
            return Ok(());
        }

        if let Some((plugin_id, session_id)) = self.0.tab_chat_backing(tab_id) {
            return self.0.chatbackend_send(&plugin_id, &session_id, prefixed);
        }

        let pty_id = self.0.resolve_pty_id(tab_id)?;
        let paste = format!("\x1b[200~{}\x1b[201~", prefixed);
        self.0
            .pty
            .lock()
            .write(&pty_id, paste.as_bytes())
            .map_err(|e| e.to_string())?;

        let pty = self.0.pty.clone();
        let pty_id = pty_id.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(500));
            let _ = pty.lock().write(&pty_id, b"\r");
            std::thread::sleep(std::time::Duration::from_millis(2000));
            let _ = pty.lock().write(&pty_id, b"\r");
        });

        Ok(())
    }

    fn queue_inbound(&self, tab_id: &str, prefixed: String) {
        codeterm_bridge::bridge::inbound_buffer::push(tab_id, prefixed);
    }

    fn claim_channel_answer(&self, callback_data: &str) -> bool {
        self.0.claim_channel_answer(callback_data)
    }

    fn telegram_token(&self) -> Option<String> {
        codeterm_bridge::bridge::telegram::read_token(&AppAttachmentStorePort)
    }

    fn telegram_token_path(&self) -> PathBuf {
        codeterm_bridge::bridge::telegram::token_path(&AppAttachmentStorePort)
    }
}

pub struct ServiceChatPreprocessorPort(pub ServiceContext);

impl ChatPreprocessorPort for ServiceChatPreprocessorPort {
    fn preprocessor_ids(&self) -> Vec<String> {
        self.0.chat_preprocessor_registry.ids()
    }

    fn active_preprocessors(&self) -> Vec<Arc<dyn ChatPreprocessorProvider>> {
        self.0.chat_preprocessor_registry.active()
    }
}

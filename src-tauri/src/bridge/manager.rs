//! BridgeManager — routes inbound messages to the General Agent PTY.

use tokio::sync::mpsc;
use super::traits::{InboundMessage, MessagingService, ReplyContext};
use crate::service::general_agent;
use crate::service::ServiceContext;

pub struct BridgeManager {
    svc: ServiceContext,
}

impl BridgeManager {
    pub fn new(svc: ServiceContext) -> Self {
        Self { svc }
    }

    pub async fn start(self, services: Vec<Box<dyn MessagingService>>) {
        let (tx, mut rx) = mpsc::channel::<InboundMessage>(64);

        for service in services {
            let tx_clone = tx.clone();
            let id = service.id().to_string();
            tokio::spawn(async move {
                log::info!("[bridge] starting service: {}", id);
                service.start(tx_clone).await;
            });
        }

        log::info!("[bridge] manager started");
        while let Some(msg) = rx.recv().await {
            log::info!("[bridge] from {} ({}): {}", msg.user_name, msg.service, &msg.text[..msg.text.len().min(100)]);

            let response = self.process_message(&msg).await;
            match response {
                Ok(text) => {
                    if let Err(e) = self.send_reply(&msg.reply_ctx, &text).await {
                        log::error!("[bridge] reply failed: {}", e);
                    }
                }
                Err(e) => {
                    log::error!("[bridge] process failed: {}", e);
                    let _ = self.send_reply(&msg.reply_ctx, &format!("Error: {}", e)).await;
                }
            }
        }
    }

    async fn process_message(&self, msg: &InboundMessage) -> Result<String, String> {
        // Find General Agent pane via alias and write message to its PTY
        let (pane_id, pty_id) = {
            let mux = self.svc.mux.lock();
            let pid = mux.resolve_pane_id(general_agent::GENERAL_ALIAS)
                .ok_or("General Agent not found")?;
            let pane = mux.get_pane(&pid).ok_or("General Agent pane missing")?;
            let pty = pane.pty_id.clone().ok_or("General Agent PTY not ready")?;
            (pid, pty)
        };

        // Raw write (not bracket paste)
        self.svc.pty.lock().write(&pty_id, format!("{}\r", msg.text).as_bytes())?;

        // Capture response
        let output_buf = self.svc.output_buf.clone();
        let tracker = self.svc.agent_tracker.clone();
        tokio::task::spawn_blocking(move || {
            super::response::capture_response(&pty_id, &output_buf, &tracker)
        }).await.map_err(|e| e.to_string())?
    }

    async fn send_reply(&self, ctx: &ReplyContext, text: &str) -> Result<(), String> {
        match ctx {
            ReplyContext::Telegram { chat_id } => {
                let token = super::telegram::read_token().ok_or("No Telegram token")?;
                let client = reqwest::Client::new();
                for chunk in super::response::chunk_message(text, super::telegram::TELEGRAM_MAX_LEN) {
                    let _ = client.post(format!("https://api.telegram.org/bot{}/sendMessage", token))
                        .json(&serde_json::json!({ "chat_id": chat_id, "text": chunk }))
                        .timeout(std::time::Duration::from_secs(10))
                        .send().await;
                }
                Ok(())
            }
        }
    }
}

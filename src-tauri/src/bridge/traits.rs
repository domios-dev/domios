//! Messaging service trait and shared types.

#[derive(Debug, Clone)]
pub struct InboundMessage {
    pub service: String,
    pub user_id: i64,
    pub user_name: String,
    pub text: String,
    pub reply_ctx: ReplyContext,
}

#[derive(Debug, Clone)]
pub enum ReplyContext {
    Telegram { chat_id: i64 },
}

pub trait MessagingService: Send + 'static {
    fn id(&self) -> &str;
    fn start(
        self: Box<Self>,
        tx: tokio::sync::mpsc::Sender<InboundMessage>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>;
}

//! Messaging service trait and shared types.

/// Content types for outbound messages.
#[derive(Debug, Clone)]
pub enum OutboundContent {
    Text {
        text: String,
    },
    Photo {
        file: String,
        caption: Option<String>,
    },
    Document {
        file: String,
        caption: Option<String>,
        filename: Option<String>,
    },
    Voice {
        file: String,
        caption: Option<String>,
    },
    Animation {
        file: String,
        caption: Option<String>,
    },
    Video {
        file: String,
        caption: Option<String>,
    },
    Sticker {
        file: String,
    },
}

/// An attachment received from a messaging service (downloaded to local disk).
#[derive(Debug, Clone)]
pub struct InboundAttachment {
    /// Local file path where the attachment is saved.
    pub path: String,
    /// Content type: "photo", "document", "voice", "video", "animation", "sticker".
    pub kind: String,
    /// Original filename (if available).
    pub filename: Option<String>,
    /// MIME type (if available).
    pub mime_type: Option<String>,
}

/// A button in an inline keyboard row.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InlineButton {
    /// Button label text.
    pub text: String,
    /// Callback data sent back when pressed.
    /// Accepts "data" from CLI/API input, serializes as "callback_data" for Telegram.
    #[serde(
        alias = "data",
        rename = "callback_data",
        skip_serializing_if = "Option::is_none"
    )]
    pub data: Option<String>,
    /// URL to open when pressed (instead of callback).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// Inline keyboard: rows of buttons.
pub type InlineKeyboard = Vec<Vec<InlineButton>>;

#[derive(Debug, Clone)]
pub struct InboundMessage {
    pub service: String,
    pub user_id: i64,
    pub user_name: String,
    pub text: String,
    pub attachments: Vec<InboundAttachment>,
    /// Telegram message_id (for reactions, replies).
    pub message_id: Option<i64>,
    /// The opaque payload of a tapped inline button, when this arrived as one.
    /// Never the visible text: what the daemon reads back is the token it sent.
    pub callback_data: Option<String>,
    pub reply_ctx: ReplyContext,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplyContext {
    Telegram {
        chat_id: i64,
        /// Forum topic the message was posted in; `None` is the main chat or general topic.
        thread_id: Option<i64>,
        /// The message this one explicitly replies to.
        reply_to_message_id: Option<i64>,
    },
}

pub trait MessagingService: Send + 'static {
    fn id(&self) -> &str;
    fn start(
        self: Box<Self>,
        tx: tokio::sync::mpsc::Sender<InboundMessage>,
        cancel: std::sync::Arc<tokio::sync::Notify>,
        status: std::sync::Arc<dyn crate::bridge::supervisor::BridgeStatusSink>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>;
}

//! Per-tab Telegram topic threads. One owner holds the tab/chat/thread binding,
//! sender identity, open authority and topic fallback for every entrance.

mod compose;
pub mod identity;
pub mod registry;
pub mod routing;
mod service;
pub mod transport;

pub use compose::TOPICS_DISABLED_NOTICE;
pub use identity::{SenderIdentity, SenderRole, ThreadTabs};
pub use registry::{ChatScope, ThreadBinding, ThreadRegistry};
pub use routing::{authorize_open, InboundOrigin, InboundRoute, ThreadCaller};
pub use service::{
    CloseOutcome, MediaDestination, OpenedThread, TabText, ThreadMenuState, ThreadService,
    TopicsState,
};
pub use transport::{TelegramTopicTransport, TopicFailure, TopicProbe, TopicTransport};

/// The bridge service whose chats carry topic threads.
pub const SERVICE: &str = "telegram";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThreadError {
    ForeignTarget { caller: String, target: String },
    UnknownTab(String),
    GeneralUsesMainChat,
    BridgeInactive,
    NoOwnerChat,
    TopicsUnavailable,
    CreationFailed(String),
}

impl ThreadError {
    pub fn is_authority(&self) -> bool {
        matches!(self, Self::ForeignTarget { .. })
    }
}

impl std::fmt::Display for ThreadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ForeignTarget { caller, target } => write!(
                f,
                "bridge thread authority: tab {caller} may open only its own Telegram thread; \
                 only the General Agent may open a thread for another tab ({target})"
            ),
            Self::UnknownTab(tab_id) => write!(f, "no live tab '{tab_id}'"),
            Self::GeneralUsesMainChat => {
                write!(
                    f,
                    "the General Agent talks in the main chat and has no thread"
                )
            }
            Self::BridgeInactive => write!(f, "the Telegram bridge is not running"),
            Self::NoOwnerChat => write!(
                f,
                "no owner chat: set integrations.telegram_allowed_users to the owner's Telegram id"
            ),
            Self::TopicsUnavailable => write!(f, "{TOPICS_DISABLED_NOTICE}"),
            Self::CreationFailed(reason) => write!(f, "Telegram topic creation failed: {reason}"),
        }
    }
}

impl std::error::Error for ThreadError {}

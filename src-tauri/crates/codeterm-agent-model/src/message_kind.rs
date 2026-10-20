#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MessageKind {
    /// A direct message from one tab (or peer) to another.
    #[default]
    Message,
    /// A hierarchy / cron report bubbling up to a parent or inbox.
    Report,
    /// A host-owned event with no reply address.
    System,
    /// A fired delayed reminder injected back into a tab.
    Reminder,
    Mesh,
    Telegram,
    Pebble,
}

impl MessageKind {
    pub fn marker_kind(self) -> &'static str {
        match self {
            Self::Message => "tab",
            Self::Report => "report",
            Self::System => "system",
            Self::Reminder => "reminder",
            Self::Mesh => "mesh",
            Self::Telegram => "telegram",
            Self::Pebble => "pebble",
        }
    }

    pub fn inbound_kind(self) -> &'static str {
        match self {
            Self::Message => "tab",
            Self::Report => "report",
            Self::System => "system",
            Self::Reminder => "reminder",
            Self::Mesh => "mesh",
            Self::Telegram => "telegram",
            Self::Pebble => "pebble",
        }
    }
}

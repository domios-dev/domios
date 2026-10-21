/// The chat message a notification jumps to. Both ids are unverified caller
/// hints: they only steer navigation, and the UI resolves them when activated.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NotificationTarget {
    pub tab_id: Option<String>,
    pub chat_message_id: Option<String>,
}

impl NotificationTarget {
    pub fn from_hints(tab_id: Option<&str>, chat_message_id: Option<&str>) -> Self {
        let clean = |value: Option<&str>| {
            value
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        };
        let tab_id = clean(tab_id);
        // A message id names a row inside one tab's transcript; without that tab it names nothing.
        let chat_message_id = tab_id.as_ref().and(clean(chat_message_id));
        Self {
            tab_id,
            chat_message_id,
        }
    }
}

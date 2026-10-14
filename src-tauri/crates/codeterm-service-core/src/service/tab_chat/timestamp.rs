use crate::agent::chat_types::RichMessage;

pub(super) fn rich_message_timestamp(rich: &RichMessage) -> Option<String> {
    rich.timestamp
        .as_ref()
        .filter(|value| !value.trim().is_empty())
        .cloned()
        .or_else(|| {
            rich.ts
                .filter(|value| *value > 0)
                .map(|value| value.to_string())
        })
}

#[cfg(test)]
mod tests;

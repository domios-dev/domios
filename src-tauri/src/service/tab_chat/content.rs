use super::strip_ansi;
use crate::agent::chat_types::ChatAttachment;

pub(super) fn extract_tool_result_text(content: Option<&serde_json::Value>) -> String {
    let text = match content {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Array(arr))
            if arr
                .iter()
                .all(|block| block.get("text").is_some_and(serde_json::Value::is_string)) =>
        {
            arr.iter()
                .map(|c| c.get("text").and_then(|v| v.as_str()).unwrap_or(""))
                .collect::<Vec<_>>()
                .join("\n")
        }
        Some(v) => v.to_string(),
        None => String::new(),
    };
    strip_ansi(&text)
}

pub(super) fn attachment_from_image(data: &serde_json::Value, id: String) -> ChatAttachment {
    let source = data.get("source");
    let mime_type = source
        .and_then(|s| s.get("media_type"))
        .and_then(|v| v.as_str())
        .unwrap_or("image/png")
        .to_string();
    let b64 = source.and_then(|s| s.get("data")).and_then(|v| v.as_str());
    ChatAttachment {
        id,
        mime_type,
        data: b64.map(String::from),
        url: source
            .and_then(|s| s.get("url"))
            .and_then(|v| v.as_str())
            .map(String::from),
        path: source
            .and_then(|s| s.get("path"))
            .and_then(|v| v.as_str())
            .map(String::from),
    }
}

#[cfg(test)]
mod tests;

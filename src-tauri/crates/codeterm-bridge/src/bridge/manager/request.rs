use crate::bridge::traits::{validate_caller_buttons, InlineKeyboard, OutboundContent};
use serde_json::Value;

pub fn outbound_content(params: &Option<Value>) -> Result<OutboundContent, String> {
    caller_buttons(params.as_ref().and_then(|params| params.get("buttons")))
        .map_err(str::to_string)?;
    let mut content_type = params
        .as_ref()
        .and_then(|p| p.get("type"))
        .and_then(|v| v.as_str())
        .unwrap_or("text")
        .to_string();

    if content_type == "text" && params.as_ref().and_then(|p| p.get("file")).is_some() {
        content_type = "document".into();
    }

    let content = match content_type.as_str() {
        "text" => {
            let text = required(params, "text")?;
            OutboundContent::Text { text }
        }
        "photo" | "document" | "voice" | "animation" | "video" | "sticker" => {
            let file = required(params, "file")?;
            if !std::path::Path::new(&file).exists() {
                return Err(format!("File not found: {}", file));
            }
            let caption = params
                .as_ref()
                .and_then(|p| p.get("caption"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .or_else(|| {
                    params
                        .as_ref()
                        .and_then(|p| p.get("text"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                });
            let filename = params
                .as_ref()
                .and_then(|p| p.get("filename"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            match content_type.as_str() {
                "photo" => OutboundContent::Photo { file, caption },
                "document" => OutboundContent::Document {
                    file,
                    caption,
                    filename,
                },
                "voice" => OutboundContent::Voice { file, caption },
                "animation" => OutboundContent::Animation { file, caption },
                "video" => OutboundContent::Video { file, caption },
                "sticker" => OutboundContent::Sticker { file },
                _ => unreachable!(),
            }
        }
        _ => {
            return Err(format!("Unknown content type: {}", content_type));
        }
    };

    Ok(content)
}

fn required(params: &Option<Value>, key: &str) -> Result<String, String> {
    params
        .as_ref()
        .and_then(|p| p.get(key))
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("Missing required parameter: {key}"))
}

/// Shared caller boundary for REST and socket bridge sends; internal notification pushes bypass it.
pub fn caller_buttons(value: Option<&Value>) -> Result<Option<InlineKeyboard>, &'static str> {
    let buttons = value
        .filter(|value| !value.is_null())
        .map(|value| {
            serde_json::from_value::<InlineKeyboard>(value.clone())
                .map_err(|_| "Invalid bridge buttons")
        })
        .transpose()?;
    validate_caller_buttons(buttons.as_ref())?;
    Ok(buttons)
}

#[cfg(test)]
mod tests;

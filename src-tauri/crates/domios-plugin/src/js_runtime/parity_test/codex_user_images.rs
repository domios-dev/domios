//! A Codex pasted-image turn (`<image name=[Image #N] path="...">`, `input_image`, `</image>`)
//! must flatten to one user row whose attachment carries the local path and the supplied bytes.

use super::*;
use crate::rich_to_chat::rich_to_chat_msgs;

const PASTED: &str =
    r"C:\Users\User\AppData\Local\Temp\codeterm-chat\e8a97274-pasted-image-1791277476989.png";

fn codex_user_turn(trailing_text: &str) -> String {
    serde_json::json!({
        "timestamp": "2026-10-06T09:04:38.306Z",
        "type": "response_item",
        "payload": {
            "type": "message",
            "role": "user",
            "content": [
                { "type": "input_text", "text": format!("<image name=[Image #1] path=\"{PASTED}\">") },
                { "type": "input_image", "image_url": "data:image/png;base64,iVBORw0KGgo=", "detail": "high" },
                { "type": "input_text", "text": "</image>" },
                { "type": "input_text", "text": trailing_text }
            ]
        }
    })
    .to_string()
}

fn user_rows(jsonl: &str) -> Vec<codeterm_types::agent::chat_types::ChatMsg> {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), jsonl).unwrap();
    let delta = load_js("codex").parse_session_delta(tmp.path(), 0);
    delta
        .messages
        .iter()
        .enumerate()
        .flat_map(|(i, rich)| rich_to_chat_msgs(rich, i))
        .filter(|message| message.kind == "user")
        .collect()
}

#[test]
fn codex_pasted_image_turn_flattens_to_one_user_row_with_a_path_attachment() {
    if require_plugins().is_none() {
        return;
    }
    let rows = user_rows(&format!("{}\n", codex_user_turn("[Image #1]")));
    assert_eq!(rows.len(), 1, "{rows:#?}");
    assert!(!rows[0].content.contains("<image"), "{:?}", rows[0].content);
    assert!(
        !rows[0].content.contains("[Image #1]"),
        "{:?}",
        rows[0].content
    );
    let attachments = rows[0].attachments.as_ref().expect("image attachment");
    assert_eq!(attachments.len(), 1);
    assert_eq!(attachments[0].mime_type, "image/png");
    assert_eq!(attachments[0].path.as_deref(), Some(PASTED));
    assert_eq!(attachments[0].data.as_deref(), Some("iVBORw0KGgo="));
}

#[test]
fn codex_captioned_image_turn_keeps_the_caption() {
    if require_plugins().is_none() {
        return;
    }
    let rows = user_rows(&format!(
        "{}\n",
        codex_user_turn("what is wrong here [Image #1]")
    ));
    assert_eq!(rows.len(), 1, "{rows:#?}");
    assert_eq!(rows[0].content, "what is wrong here");
    assert_eq!(
        rows[0].attachments.as_ref().map(|a| a[0].path.clone()),
        Some(Some(PASTED.to_string()))
    );
}

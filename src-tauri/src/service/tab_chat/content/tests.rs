use super::*;
use serde_json::json;

#[test]
fn image_tool_result_preserves_bytes_and_text() {
    let image = json!({"type":"image","source":{"type":"base64","media_type":"image/jpeg","data":"opaque-image-data"}});
    for blocks in [
        json!([image]),
        json!([{"type":"text","text":"opaque-output"}, image]),
    ] {
        let output = extract_tool_result_text(Some(&blocks));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&output).unwrap(),
            blocks
        );
    }
}

#[test]
fn text_tool_results_keep_plain_text_and_strip_ansi() {
    let colored = format!("{esc}[31msecond{esc}[0m", esc = char::from(27));
    assert_eq!(
        extract_tool_result_text(Some(
            &json!([{"text":"first"},{"type":"text","text":colored}])
        )),
        "first\nsecond"
    );
    assert_eq!(extract_tool_result_text(Some(&json!("opaque"))), "opaque");
    assert_eq!(extract_tool_result_text(None), "");
}

#[test]
fn user_image_keeps_payload_even_with_a_path_above_old_limit() {
    let bytes = "A".repeat(100_001);
    let image = json!({"source":{"media_type":"image/png","data":bytes,"path":"/tmp/opaque.png"}});
    let attachment = attachment_from_image(&image, "opaque-id".into());
    assert_eq!(attachment.data.as_deref(), Some(bytes.as_str()));
    assert_eq!(attachment.path.as_deref(), Some("/tmp/opaque.png"));
    assert_eq!(attachment.mime_type, "image/png");
}

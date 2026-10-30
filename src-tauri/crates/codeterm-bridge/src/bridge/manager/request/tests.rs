use super::*;

#[test]
fn caller_bridge_buttons_refuse_reserved_tokens_on_text_and_media_sends() {
    for key in ["data", "callback_data"] {
        for token in ["na:question:0", "na:", " \nna:question:0 "] {
            let mut button = serde_json::json!({"text":"Show log"});
            button[key] = token.into();
            let buttons = serde_json::json!([[button]]);
            assert!(caller_buttons(Some(&buttons)).is_err());
            for kind in [
                "text",
                "photo",
                "document",
                "voice",
                "animation",
                "video",
                "sticker",
            ] {
                let error = outbound_content(&Some(
                    serde_json::json!({"type":kind,"text":"Log","buttons":buttons}),
                ))
                .unwrap_err();
                assert_eq!(error, "Notification answer button data is reserved");
            }
        }
    }
    assert!(caller_buttons(Some(&serde_json::json!([[{"text":"Log","data":"log"},{"text":"Docs","url":"https://example.invalid"}]]))).is_ok());
}

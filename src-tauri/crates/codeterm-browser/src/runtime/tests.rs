use super::*;

#[test]
fn browser_runtime_version_endpoint_requires_loopback_ws() {
    assert_eq!(
        version_endpoint(&json!({
            "Browser": "Edg/141.0.0.0",
            "webSocketDebuggerUrl": "ws://127.0.0.1:9223/devtools/browser/0b4e"
        })),
        Ok("ws://127.0.0.1:9223/devtools/browser/0b4e".to_string())
    );
    assert!(version_endpoint(&json!({"webSocketDebuggerUrl": "ws://10.0.0.2:9223/x"})).is_err());
    assert!(version_endpoint(&json!({"Browser": "Chrome"})).is_err());
}

#[test]
fn browser_runtime_carries_only_web_pages_across_profile_switch() {
    assert_eq!(
        carry_url("https://mail.example.com/inbox?x=1"),
        Some("https://mail.example.com/inbox?x=1".into())
    );
    assert_eq!(
        carry_url("http://127.0.0.1:5173/"),
        Some("http://127.0.0.1:5173/".into())
    );
    assert_eq!(carry_url("about:blank"), None);
    assert_eq!(carry_url("chrome-error://chromewebdata/"), None);
    assert_eq!(carry_url("data:text/html,hi"), None);
    assert_eq!(carry_url(""), None);
}

#[test]
fn browser_runtime_profile_label_names_attach_mode() {
    let attached = BrowserMode::Attached {
        http_base: "http://127.0.0.1:9223".into(),
    };
    assert_eq!(attached.profile_label(), "attached");
    let named = BrowserMode::Launched {
        profile: ProfileKind::Named {
            name: "work".into(),
        },
    };
    assert_eq!(named.profile_label(), "work");
}

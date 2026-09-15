//! `codeterm bridge thread open`: open or reuse a tab's Telegram topic.

use serde_json::{json, Value};

use crate::args::misc::BridgeThreadAction;
use crate::client::SocketClient;
use crate::cmd::print::handle_result;

/// The daemon decides authority from `caller_tab_id`; `tab_id` is only the target.
pub(crate) fn open_params(service: &str, tab: Option<&str>, caller_tab_id: &str) -> Value {
    let mut params = json!({ "service": service, "caller_tab_id": caller_tab_id });
    if let Some(tab) = tab.map(str::trim).filter(|tab| !tab.is_empty()) {
        params["tab_id"] = json!(tab);
    }
    params
}

/// One sentence a General Agent can relay verbatim. Without a link the topic title is the pointer.
pub(crate) fn render_opened(value: &Value) -> String {
    let topic = value
        .get("topic_name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match value.get("link").and_then(Value::as_str) {
        Some(link) => format!("Telegram topic \"{topic}\" is ready: {link}"),
        None => format!("Telegram topic \"{topic}\" is ready in the bot chat's topic list."),
    }
}

pub fn handle_thread(client: &mut SocketClient, json: bool, action: BridgeThreadAction) {
    match action {
        BridgeThreadAction::Open { tab, service } => {
            let caller = crate::cmd::local_io::resolve_caller_tab(None, "bridge thread open");
            let result = client.call(
                "bridge.thread.open",
                Some(open_params(&service, tab.as_deref(), &caller)),
            );
            match result {
                Ok(value) if !json => println!("{}", render_opened(&value)),
                other => handle_result(json, other),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct BridgeOnly {
        #[command(subcommand)]
        action: crate::BridgeAction,
    }

    #[test]
    fn telegram_topic_attach_params_default_to_the_caller_tab() {
        assert_eq!(
            open_params("telegram", None, "tab_self"),
            json!({ "service": "telegram", "caller_tab_id": "tab_self" })
        );
        assert_eq!(
            open_params("telegram", Some("   "), "tab_self"),
            json!({ "service": "telegram", "caller_tab_id": "tab_self" }),
            "a blank --tab names no target"
        );
    }

    #[test]
    fn telegram_topic_attach_params_carry_an_explicit_target_beside_the_caller() {
        assert_eq!(
            open_params("telegram", Some("tab_target"), "tab_general"),
            json!({
                "service": "telegram",
                "caller_tab_id": "tab_general",
                "tab_id": "tab_target",
            })
        );
    }

    #[test]
    fn telegram_topic_attach_parses_tab_and_service() {
        let parsed = BridgeOnly::try_parse_from(["bridge", "thread", "open", "--tab", "abc123"])
            .expect("thread open parses");
        match parsed.action {
            crate::BridgeAction::Thread {
                action: BridgeThreadAction::Open { tab, service },
            } => {
                assert_eq!(tab.as_deref(), Some("abc123"));
                assert_eq!(service, "telegram");
            }
            _ => panic!("expected bridge thread open"),
        }

        let own = BridgeOnly::try_parse_from(["bridge", "thread", "open", "--service", "telegram"])
            .expect("thread open without --tab parses");
        assert!(matches!(
            own.action,
            crate::BridgeAction::Thread {
                action: BridgeThreadAction::Open { tab: None, .. }
            }
        ));
    }

    #[test]
    fn telegram_topic_attach_render_is_one_line_carrying_the_link_when_present() {
        let link = "https://t.me/c/1234567890/42";
        let rendered = render_opened(&json!({
            "topic_name": "topic_fixture_a",
            "link": link,
            "thread_id": 42,
        }));
        assert!(rendered.contains(link) && rendered.contains("topic_fixture_a"));
        assert!(!rendered.contains('\n'));
    }

    #[test]
    fn telegram_topic_attach_render_without_a_link_names_the_topic_and_no_url() {
        let rendered = render_opened(&json!({
            "topic_name": "topic_fixture_b",
            "link": null,
            "thread_id": 42,
        }));
        assert!(rendered.contains("topic_fixture_b"));
        assert!(!rendered.contains("t.me") && !rendered.contains("://"));
        assert!(!rendered.contains("42") && !rendered.contains('\n'));
    }
}

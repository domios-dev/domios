use crate::args::agent_runtime::{AgentUsageAction, ChildHistoryArgs};
use crate::local_api::{LocalApiClient, LocalApiError, LocalApiResponse};

pub fn handle_usage(http: &LocalApiClient, base: &str, json: bool, action: AgentUsageAction) {
    match action {
        AgentUsageAction::Read { tab } => {
            let url = format!(
                "{}/api/tabs/{}/agent/runtime/usage",
                base,
                urlencoding::encode(&tab)
            );
            let usage = request_value(http.get(url).send());
            print_value(serde_json::json!({"tab_id":tab,"usage":usage}), json);
        }
        AgentUsageAction::Reset { tab, confirm: true } => {
            let url = format!(
                "{}/api/tabs/{}/agent/runtime/reset-credits",
                base,
                urlencoding::encode(&tab)
            );
            let body = serde_json::json!({
                "action": "redeem",
                "idempotencyKey": uuid::Uuid::new_v4(),
                "confirmed": true
            });
            print_response(http.post(url).json(&body).send(), json);
        }
        AgentUsageAction::Reset { confirm: false, .. } => {
            crate::cmd::print::fail(2, "reset requires --confirm");
        }
    }
}

fn closed_children_url(base: &str, parent: &str, child: Option<&str>, since: usize) -> String {
    let list = format!(
        "{}/api/tabs/{}/closed-children",
        base,
        urlencoding::encode(parent)
    );
    match child {
        Some(child) => format!("{list}?child={}&since={since}", urlencoding::encode(child)),
        None => list,
    }
}

fn closed_child_line(child: &serde_json::Value) -> String {
    let text = |key: &str| {
        child
            .get(key)
            .and_then(|value| value.as_str())
            .unwrap_or("")
    };
    let model = match text("model") {
        "" => text("provider"),
        model => model,
    };
    format!("{}  {} [{}]", text("tab_id"), text("title"), model)
}

fn print_closed_children(value: &serde_json::Value) {
    let children = value
        .get("children")
        .and_then(|children| children.as_array())
        .map(Vec::as_slice)
        .unwrap_or_default();
    if children.is_empty() {
        println!("(no closed children)");
    }
    for child in children {
        println!("{}", closed_child_line(child));
    }
}

fn handle_closed_children(http: &LocalApiClient, base: &str, json: bool, args: ChildHistoryArgs) {
    let url = closed_children_url(base, &args.tab, args.child.as_deref(), args.since);
    let value = request_value(http.get(url).send());
    if json {
        print_value(value, true);
    } else if args.child.is_some() {
        super::tab::print_tab_chat(&value);
    } else {
        print_closed_children(&value);
    }
}

pub fn handle_child_history(http: &LocalApiClient, base: &str, json: bool, args: ChildHistoryArgs) {
    if args.closed {
        return handle_closed_children(http, base, json, args);
    }
    let (Some(thread), Some(root_thread_id), Some(binding_nonce), Some(epoch)) = (
        args.thread.as_deref(),
        args.root_thread_id.as_deref(),
        args.binding_nonce.as_deref(),
        args.epoch,
    ) else {
        crate::cmd::print::fail(
            2,
            "child-history requires --closed or a recorded thread binding",
        );
    };
    let mut url = format!(
        "{}/api/tabs/{}/agent/runtime/children/{}/history?rootThreadId={}&bindingNonce={}&epoch={}",
        base,
        urlencoding::encode(&args.tab),
        urlencoding::encode(thread),
        urlencoding::encode(root_thread_id),
        urlencoding::encode(binding_nonce),
        epoch
    );
    if let Some(cursor) = args.cursor {
        url.push_str("&cursor=");
        url.push_str(&urlencoding::encode(&cursor));
    }
    print_response(http.get(url).send(), json);
}

fn print_response(response: Result<LocalApiResponse, LocalApiError>, json: bool) {
    print_value(request_value(response), json);
}

fn request_value(response: Result<LocalApiResponse, LocalApiError>) -> serde_json::Value {
    let response = match response {
        Ok(response) => response,
        Err(error) => crate::cmd::print::fail(1, error.to_string()),
    };
    let status = response.status();
    let body = response.text().unwrap_or_default();
    let value: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
    if !status.is_success() {
        crate::cmd::print::fail_http(1, status.as_u16(), &body);
    }
    value
}

fn print_value(value: serde_json::Value, json: bool) {
    if json {
        println!("{}", crate::cmd::output::encode(&value).unwrap_or_default());
    } else {
        println!(
            "{}",
            serde_json::to_string_pretty(&value).unwrap_or_default()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_child_urls_encode_their_tab_ids() {
        assert_eq!(
            closed_children_url("http://host", "par/ent", None, 0),
            "http://host/api/tabs/par%2Fent/closed-children"
        );
        assert_eq!(
            closed_children_url("http://host", "parent", Some("ch ild"), 4),
            "http://host/api/tabs/parent/closed-children?child=ch%20ild&since=4"
        );
    }

    #[test]
    fn a_closed_child_row_names_its_model_or_else_its_provider() {
        assert_eq!(
            closed_child_line(&serde_json::json!({
                "tab_id": "tab-a", "title": "Ada", "provider": "acp", "model": "DeepSeek-V4-Pro"
            })),
            "tab-a  Ada [DeepSeek-V4-Pro]"
        );
        assert_eq!(
            closed_child_line(&serde_json::json!({
                "tab_id": "tab-b", "title": "Ezra", "provider": "acp"
            })),
            "tab-b  Ezra [acp]"
        );
    }
}

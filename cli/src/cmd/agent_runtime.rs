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

pub fn handle_child_history(http: &LocalApiClient, base: &str, json: bool, args: ChildHistoryArgs) {
    let mut url = format!(
        "{}/api/tabs/{}/agent/runtime/children/{}/history?rootThreadId={}&bindingNonce={}&epoch={}",
        base,
        urlencoding::encode(&args.tab),
        urlencoding::encode(&args.thread),
        urlencoding::encode(&args.root_thread_id),
        urlencoding::encode(&args.binding_nonce),
        args.epoch
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

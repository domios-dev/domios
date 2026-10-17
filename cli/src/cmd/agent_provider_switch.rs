#[derive(serde::Serialize)]
pub(crate) struct SwitchProviderRequest {
    pub provider: String,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub force: bool,
}

pub(crate) fn run(
    client: &crate::local_api::LocalApiClient,
    base: &str,
    tab: &str,
    request: SwitchProviderRequest,
    json: bool,
) {
    let response = client
        .post(format!(
            "{base}/api/tabs/{}/agent/switch-provider",
            urlencoding::encode(tab)
        ))
        .json(&request)
        .send()
        .unwrap_or_else(|error| crate::cmd::print::fail(1, error.to_string()));
    let status = response.status();
    let text = response.text().unwrap_or_default();
    if !status.is_success() {
        crate::cmd::print::fail_http(1, status.as_u16(), &text);
    }
    let result: serde_json::Value = serde_json::from_str(&text)
        .unwrap_or_else(|error| crate::cmd::print::fail(1, error.to_string()));
    if result["handoff_delivered"] != true || result["binding_restored"] != true {
        crate::cmd::print::fail(
            1,
            "provider switch did not confirm handoff and session binding",
        );
    }
    if json {
        crate::cmd::print::print_json(&result);
    } else {
        println!(
            "Switched tab {} to {} (session {}). Previous transcript: {}",
            tab,
            request.provider,
            result["session_id"].as_str().unwrap_or("unknown"),
            result["previous_history_path"]
                .as_str()
                .unwrap_or("unavailable")
        );
    }
}

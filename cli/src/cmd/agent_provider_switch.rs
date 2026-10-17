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
    if result["phase"] != "pending" || result["operation_id"].as_str().is_none() {
        crate::cmd::print::fail(1, "provider switch was not accepted");
    }
    if json {
        crate::cmd::print::print_json(&result);
    } else {
        println!("Accepted provider switch {} for tab {}. Read: codeterm agent switch-provider-status {} --tab {}", result["operation_id"].as_str().unwrap(), tab, result["operation_id"].as_str().unwrap(), tab);
    }
}

pub(crate) fn status(
    client: &crate::local_api::LocalApiClient,
    base: &str,
    tab: &str,
    operation: &str,
    json: bool,
) {
    let response = client
        .get(format!(
            "{base}/api/tabs/{}/agent/switch-provider/{}",
            urlencoding::encode(tab),
            urlencoding::encode(operation)
        ))
        .send()
        .unwrap_or_else(|error| crate::cmd::print::fail(1, error.to_string()));
    let code = response.status();
    let text = response.text().unwrap_or_default();
    if !code.is_success() {
        crate::cmd::print::fail_http(1, code.as_u16(), &text);
    }
    let result: serde_json::Value = serde_json::from_str(&text)
        .unwrap_or_else(|error| crate::cmd::print::fail(1, error.to_string()));
    if json {
        crate::cmd::print::print_json(&result);
    } else {
        println!("{}", serde_json::to_string_pretty(&result).unwrap());
    }
}

use serde_json::json;
use std::io::Read;

fn read_key(input: impl Read) -> Result<String, String> {
    let mut key = String::new();
    input
        .take(16385)
        .read_to_string(&mut key)
        .map_err(|_| "Could not read API key from stdin")?;
    if key.len() > 16384 {
        return Err("API key input exceeds 16 KiB".into());
    }
    let key = key.trim().to_string();
    if key.is_empty() || key.len() > 16384 {
        return Err("API key must contain 1–16384 bytes".into());
    }
    Ok(key)
}
pub(super) struct LoginOptions {
    pub device: bool,
    pub method: Option<String>,
    pub api_key_stdin: bool,
    pub cancel: bool,
    pub show: bool,
}

/// The sign-in request without credentials; a declared method id selects the agent's own flow.
pub(crate) fn request_body(device: bool, method: Option<&str>, cancel: bool) -> serde_json::Value {
    if cancel {
        json!({"action":"cancel"})
    } else if let Some(method) = method {
        json!({"methodId":method})
    } else {
        json!({"action":"start","method":if device {"device"} else {"browser"}})
    }
}

/// The daemon's own refusal text, bounded, in place of a generic failure.
fn refusal(status: u16, body: &str) -> String {
    let extracted = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|parsed| {
            parsed
                .pointer("/reason")
                .or_else(|| parsed.pointer("/error/message"))
                .or_else(|| parsed.pointer("/message"))
                .and_then(|value| value.as_str())
                .map(str::to_owned)
        });
    let text = extracted.as_deref().unwrap_or(body);
    let reason: String = text.trim().chars().take(512).collect();
    if reason.is_empty() {
        format!("Sign-in refused (HTTP {status}). Check the tab's sign-in banner.")
    } else {
        format!("Sign-in refused: {reason}")
    }
}

pub(super) fn run(
    client: &crate::local_api::LocalApiClient,
    base: &str,
    json_output: bool,
    tab: String,
    options: LoginOptions,
) {
    let LoginOptions {
        device,
        method,
        api_key_stdin,
        cancel,
        show,
    } = options;
    let mut request = request_body(device, method.as_deref(), cancel);
    if api_key_stdin && method.is_none() {
        request["method"] = "apiKey".into();
    }
    if api_key_stdin {
        match read_key(std::io::stdin().lock()) {
            Ok(key) => request["apiKey"] = key.into(),
            Err(error) => crate::cmd::print::fail(1, error),
        }
    }
    let endpoint = format!(
        "{base}/api/tabs/{}/agent/runtime/login",
        urlencoding::encode(&tab)
    );
    let response = if show {
        client.get(endpoint).send()
    } else {
        client.post(endpoint).json(&request).send()
    };
    let response = match response {
        Ok(response) => response,
        Err(_) => crate::cmd::print::fail(1, "Sign-in request could not reach the daemon"),
    };
    let status = response.status();
    if !status.is_success() {
        let body = response.text().unwrap_or_default();
        crate::cmd::print::fail(1, refusal(status.as_u16(), &body));
    }
    let value: serde_json::Value = match response.json() {
        Ok(value) => value,
        Err(_) => crate::cmd::print::fail(1, "Invalid sign-in response"),
    };
    print!("{}", render(&value, json_output));
}

fn render(value: &serde_json::Value, json_output: bool) -> String {
    use std::fmt::Write;
    let mut output = String::new();
    if json_output {
        return format!("{value}\n");
    }
    let status = value
        .pointer("/codeSubmission/status")
        .or_else(|| value.pointer("/outcome"))
        .or_else(|| value.pointer("/login/status"))
        .and_then(|value| value.as_str())
        .unwrap_or("requested");
    let _ = writeln!(output, "Sign-in {status}");
    if let Some(login_id) = value
        .pointer("/login/loginId")
        .and_then(|value| value.as_str())
    {
        let _ = writeln!(output, "Login ID: {login_id}");
    }
    if let Some(url) = value.pointer("/login/url").and_then(|value| value.as_str()) {
        let _ = writeln!(output, "Open in your system browser: {url}");
    }
    if let Some(code) = value
        .pointer("/login/userCode")
        .and_then(|value| value.as_str())
    {
        let _ = writeln!(output, "Device code: {code}");
    }
    if let Some(expiry) = value.pointer("/login/expiresAtMs").and_then(|v| v.as_u64()) {
        let _ = writeln!(output, "Expires at (Unix milliseconds): {expiry}");
    } else if value.pointer("/login/status").and_then(|v| v.as_str()) == Some("pending") {
        let _ = writeln!(output, "Expiry: not reported by provider");
    }
    let expects_code = value
        .pointer("/login/codeInput")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
        || value
            .pointer("/state/loginCodeInput")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
    if expects_code {
        let _ = writeln!(
            output,
            "Authorization code required: submit the single-use code in the Domios UI sign-in card (never via CLI or chat)."
        );
    }
    if let Some(state) = value
        .pointer("/authentication/state")
        .or_else(|| value.pointer("/state/auth"))
        .and_then(|v| v.as_str())
    {
        let _ = writeln!(output, "Authentication: {state}");
    }
    let choices = value
        .pointer("/state/loginChoices")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if !choices.is_empty() {
        let _ = writeln!(output, "Provider sign-in choices (use --method ID):");
        for choice in choices {
            let id = choice
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let label = choice.get("label").and_then(|v| v.as_str()).unwrap_or(id);
            let _ = writeln!(output, "  {id}  {label}");
        }
    }
    let key_methods = value
        .get("apiKeyMethods")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let methods = value
        .pointer("/authentication/methods")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if !methods.is_empty() {
        let _ = writeln!(output, "Declared sign-in methods (use --method ID):");
    }
    for method in methods {
        let id = method
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let name = method.get("name").and_then(|v| v.as_str()).unwrap_or(id);
        let mut notes = Vec::new();
        if key_methods.iter().any(|key| key.as_str() == Some(id)) {
            notes.push("API key via --api-key-stdin");
        }
        if method.get("available").and_then(|v| v.as_bool()) == Some(false) {
            notes.push("unavailable here");
        }
        let notes = if notes.is_empty() {
            String::new()
        } else {
            format!(" ({})", notes.join("; "))
        };
        let _ = writeln!(output, "  {id}  {name}{notes}");
    }
    output
}
#[cfg(test)]
#[path = "agent_login/tests.rs"]
mod tests;
